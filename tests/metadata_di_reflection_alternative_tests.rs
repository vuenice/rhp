use std::collections::HashMap;

use rhp::ast::types::Visibility;
use rhp::ir::compile_source;
use rhp::metadata::{
    detect_circular_dependencies, generate_factories, Container, DiError, DispatchError,
    HandlerShim, ParamSource,
};
use rhp::type_system::types::RhpType;
use rhp::vm::engine::VM;
use rhp::vm::value::{PhpValue, VmObject};
use rhp::vm::worker::WorkerRequest;

#[test]
fn test_metadata_table_emitted_at_compile_time() {
    let source = r#"
        #[Route('/api/orders')]
        class OrderController {
            public string $name;

            #[Inject]
            public function __construct(PaymentGateway $gateway, Logger $logger) {
                $this->name = "orders";
            }

            #[Get('/{id}')]
            public function show(int $id): string {
                return "order_" . $id;
            }
        }

        class PaymentGateway {}
        class Logger {}
    "#;

    let module = compile_source(source).expect("Compilation must succeed");
    let metadata = &module.metadata;

    // Verify ClassMetadata
    let order_ctrl = metadata.get_class("OrderController").expect("OrderController must exist in metadata");
    assert_eq!(order_ctrl.name, "OrderController");
    assert!(order_ctrl.has_attribute("Route"));
    let route_attr = order_ctrl.get_attribute("Route").unwrap();
    assert_eq!(route_attr.first_str(), Some("/api/orders"));

    // Verify Constructor Param Metadata
    let ctor = order_ctrl.constructor.as_ref().expect("Constructor must exist in metadata");
    assert_eq!(ctor.name, "__construct");
    assert!(ctor.has_attribute("Inject"));
    assert_eq!(ctor.params.len(), 2);
    assert_eq!(ctor.params[0].name, "$gateway");
    assert_eq!(ctor.params[0].rhp_type, RhpType::Class("PaymentGateway".to_string()));
    assert_eq!(ctor.params[1].name, "$logger");
    assert_eq!(ctor.params[1].rhp_type, RhpType::Class("Logger".to_string()));

    // Verify Method Metadata & Attributes
    let show_method = order_ctrl.methods.get("show").expect("show method must exist");
    assert_eq!(show_method.name, "show");
    assert!(show_method.has_attribute("Get"));
    assert_eq!(show_method.params.len(), 1);
    assert_eq!(show_method.params[0].name, "$id");
    assert_eq!(show_method.params[0].rhp_type, RhpType::Int);
    assert_eq!(show_method.return_type, RhpType::String);

    // Verify Property Metadata
    let prop = order_ctrl.properties.get("name").expect("Property name must exist");
    assert_eq!(prop.name, "name");
    assert_eq!(prop.rhp_type, RhpType::String);
    assert_eq!(prop.visibility, Visibility::Public);
}

#[test]
fn test_dependency_injection_compile_time_factories() {
    let source = r#"
        class DatabaseConnection {
            public string $dsn;
            public function __construct() {
                $this->dsn = "sqlite::memory:";
            }
        }

        class UserRepository {
            public DatabaseConnection $db;
            public function __construct(DatabaseConnection $db) {
                $this->db = $db;
            }
        }

        class UserService {
            public UserRepository $users;
            public function __construct(UserRepository $users) {
                $this->users = $users;
            }
        }
    "#;

    let module = compile_source(source).expect("Compilation must succeed");
    let metadata = &module.metadata;

    // Compiler generates factories for all concrete classes
    let factories = generate_factories(metadata).expect("Factory generation must succeed");
    assert!(factories.contains_key("DatabaseConnection"));
    assert!(factories.contains_key("UserRepository"));
    assert!(factories.contains_key("UserService"));

    let user_service_factory = &factories["UserService"];
    assert_eq!(user_service_factory.dependencies.len(), 1);
    assert_eq!(user_service_factory.dependencies[0].service_type, "UserRepository");

    // Container auto-wires classes without runtime reflection
    let mut container = Container::new();
    container.auto_wire(metadata).expect("Auto-wire must succeed");

    let mut vm = VM::new(module, false);
    let user_service_val = container.resolve("UserService", &mut vm).expect("UserService resolution must succeed");

    if let PhpValue::Object(service_obj) = user_service_val {
        let repo_val = service_obj.borrow().get_property("users").cloned().expect("users property must exist");
        if let PhpValue::Object(repo_obj) = repo_val {
            let db_val = repo_obj.borrow().get_property("db").cloned().expect("db property must exist");
            if let PhpValue::Object(db_obj) = db_val {
                let dsn = db_obj.borrow().get_property("dsn").cloned().expect("dsn property must exist");
                assert_eq!(dsn, PhpValue::string("sqlite::memory:"));
            } else {
                panic!("Expected DatabaseConnection object");
            }
        } else {
            panic!("Expected UserRepository object");
        }
    } else {
        panic!("Expected UserService object");
    }
}

#[test]
fn test_compile_time_circular_dependency_error() {
    let source = r#"
        class ServiceA {
            public function __construct(ServiceB $b) {}
        }

        class ServiceB {
            public function __construct(ServiceC $c) {}
        }

        class ServiceC {
            public function __construct(ServiceA $a) {}
        }
    "#;

    let module = compile_source(source).expect("Compilation must succeed");
    let err = detect_circular_dependencies(&module.metadata).unwrap_err();

    match err {
        DiError::CircularDependency { cycle } => {
            // Must identify the circular cycle
            assert!(cycle.contains(&"ServiceA".to_string()));
            assert!(cycle.contains(&"ServiceB".to_string()));
            assert!(cycle.contains(&"ServiceC".to_string()));
        }
        other => panic!("Expected CircularDependency error, got: {:?}", other),
    }
}

#[test]
fn test_method_injection_and_handler_shim_resolution() {
    let source = r#"
        class User {
            public int $id;
            public string $name;

            public static function resolveRouteBinding(string $value): ?User {
                if ($value == "42") {
                    $u = new User();
                    $u->id = 42;
                    $u->name = "Alice";
                    return $u;
                }
                return null;
            }
        }

        class Mailer {
            public string $driver = "smtp";
        }

        class UserController {
            public function show(Request $req, User $user, int $postId, Mailer $mailer): string {
                return "User " . $user->name . " post " . $postId . " with " . $mailer->driver;
            }
        }
    "#;

    let module = compile_source(source).expect("Compilation must succeed");
    let metadata = &module.metadata;

    // Pre-compile handler shim once when route is registered
    let shim = HandlerShim::compile(
        "UserController",
        "show",
        "/users/{user}/posts/{postId}",
        metadata,
        None,
    )
    .expect("Handler shim compilation must succeed");

    // Verify parameter sources analyzed at build time:
    // Param 0 ($req) -> Request
    // Param 1 ($user) -> RouteModel via resolveRouteBinding
    // Param 2 ($postId) -> RouteParam scalar int
    // Param 3 ($mailer) -> Container
    assert_eq!(shim.param_sources.len(), 4);
    assert_eq!(
        shim.param_sources[0],
        ParamSource::Request {
            is_form_request: false,
            form_request_class: None,
        }
    );
    assert_eq!(
        shim.param_sources[1],
        ParamSource::RouteModel {
            param_name: "user".to_string(),
            model_class: "User".to_string(),
        }
    );
    assert_eq!(
        shim.param_sources[2],
        ParamSource::RouteParam {
            param_name: "postId".to_string(),
            target_type: RhpType::Int,
        }
    );
    assert_eq!(
        shim.param_sources[3],
        ParamSource::Container {
            service_type: "Mailer".to_string(),
        }
    );

    let mut container = Container::new();
    container.auto_wire(metadata).expect("Auto-wire must succeed");

    let mut vm = VM::new(module, false);
    let request = WorkerRequest::new("");

    // Test 1: Successful direct dispatch
    let mut route_params = HashMap::new();
    route_params.insert("user".to_string(), "42".to_string());
    route_params.insert("postId".to_string(), "100".to_string());

    let result = shim
        .dispatch(&mut vm, &mut container, &request, &route_params)
        .expect("Dispatch must succeed");

    assert_eq!(result, PhpValue::string("User Alice post 100 with smtp"));

    // Test 2: RouteModel 404 Not Found when resolveRouteBinding returns null
    let mut not_found_params = HashMap::new();
    not_found_params.insert("user".to_string(), "999".to_string());
    not_found_params.insert("postId".to_string(), "100".to_string());

    let err = shim
        .dispatch(&mut vm, &mut container, &request, &not_found_params)
        .unwrap_err();

    match err {
        DispatchError::NotFound { model, param_name, value } => {
            assert_eq!(model, "User");
            assert_eq!(param_name, "user");
            assert_eq!(value, "999");
        }
        other => panic!("Expected NotFound, got: {:?}", other),
    }

    // Test 3: Scalar type validation failure (passing non-integer to int $postId)
    let mut invalid_params = HashMap::new();
    invalid_params.insert("user".to_string(), "42".to_string());
    invalid_params.insert("postId".to_string(), "not-a-number".to_string());

    let err2 = shim
        .dispatch(&mut vm, &mut container, &request, &invalid_params)
        .unwrap_err();

    match err2 {
        DispatchError::ValidationError { param_name, expected, actual } => {
            assert_eq!(param_name, "postId");
            assert_eq!(expected, "integer");
            assert_eq!(actual, "not-a-number");
        }
        other => panic!("Expected ValidationError, got: {:?}", other),
    }
}

#[test]
fn test_container_swapping_for_zero_reflection_testing() {
    let source = r#"
        class StripeGateway {
            public function charge(): string {
                return "charged_live_stripe";
            }
        }

        class FakeGateway extends StripeGateway {
            public function charge(): string {
                return "charged_fake_mock";
            }
        }

        class OrderCheckout {
            public StripeGateway $gateway;
            public function __construct(StripeGateway $gateway) {
                $this->gateway = $gateway;
            }

            public function process(): string {
                return $this->gateway->charge();
            }
        }
    "#;

    let module = compile_source(source).expect("Compilation must succeed");
    let mut container = Container::new();
    container.auto_wire(&module.metadata).expect("Auto-wire must succeed");

    let mut vm = VM::new(module.clone(), false);

    // Default resolution: live StripeGateway
    let checkout = container.resolve("OrderCheckout", &mut vm).expect("Resolution must succeed");
    let live_res = vm.invoke_method_sync(checkout, "process", Vec::new(), rhp::lexer::span::Span::default()).unwrap();
    assert_eq!(live_res, PhpValue::string("charged_live_stripe"));

    // Container swap: swap StripeGateway with FakeGateway for tests!
    let mut fake_obj = VmObject::new("FakeGateway");
    fake_obj.set_property("driver", PhpValue::string("fake"));
    container.swap("StripeGateway", PhpValue::object(fake_obj));

    // Resolve OrderCheckout again: automatically receives FakeGateway!
    let test_checkout = container.resolve("OrderCheckout", &mut vm).expect("Resolution must succeed");
    let mock_res = vm.invoke_method_sync(test_checkout, "process", Vec::new(), rhp::lexer::span::Span::default()).unwrap();
    assert_eq!(mock_res, PhpValue::string("charged_fake_mock"));
}

#[test]
fn test_app_facade_container_integration_in_script() {
    let source = r#"
        class NotificationService {
            public string $channel = "email";
        }

        class MockNotificationService {
            public string $channel = "mock_memory";
        }

        // Test App::swap in script
        App::swap('NotificationService', new MockNotificationService());

        $resolved = App::make('NotificationService');
        echo $resolved->channel;
    "#;

    let module = compile_source(source).expect("Compilation must succeed");
    let mut vm = VM::new(module, false);
    let _ = vm.run().expect("Execution must succeed");

    assert_eq!(vm.output(), "mock_memory");
}

#[test]
fn test_build_time_attribute_registry_discovery() {
    let source = r#"
        #[Route('/api/v1')]
        #[Policy('Post')]
        class PostController {
            #[Get('/posts', name: 'posts.index')]
            public function index(): string {
                return "index";
            }

            #[Post('/posts', middleware: ['auth', 'throttle'])]
            public function store(): string {
                return "store";
            }

            #[Command('posts:cleanup', description: 'Prune old posts')]
            public function cleanup(): void {}

            #[Listener('PostPublished')]
            public function onPostPublished(): void {}
        }

        #[mockable]
        interface CacheContract {}
    "#;

    let module = compile_source(source).expect("Compilation must succeed");
    let registry = &module.metadata.attributes;

    // Discovered routes
    assert_eq!(registry.routes.len(), 2);
    let route_get = registry
        .routes
        .iter()
        .find(|r| r.methods.contains(&"GET".to_string()))
        .expect("GET route must exist");
    assert_eq!(route_get.path, "/api/v1/posts");
    assert_eq!(route_get.controller_class, "PostController");
    assert_eq!(route_get.method_name, "index");
    assert_eq!(route_get.route_name, Some("posts.index".to_string()));

    let route_post = registry
        .routes
        .iter()
        .find(|r| r.methods.contains(&"POST".to_string()))
        .expect("POST route must exist");
    assert_eq!(route_post.path, "/api/v1/posts");
    assert_eq!(
        route_post.middleware,
        vec!["auth".to_string(), "throttle".to_string()]
    );

    // Discovered command
    assert_eq!(registry.commands.len(), 1);
    assert_eq!(registry.commands[0].signature, "posts:cleanup");
    assert_eq!(registry.commands[0].class_name, "PostController");
    assert_eq!(registry.commands[0].description, Some("Prune old posts".to_string()));

    // Discovered listener
    assert_eq!(registry.listeners.len(), 1);
    assert_eq!(registry.listeners[0].event_name, "PostPublished");
    assert_eq!(registry.listeners[0].listener_class, "PostController");
    assert_eq!(registry.listeners[0].method_name, "onPostPublished");

    // Discovered policy
    assert_eq!(registry.policies.len(), 1);
    assert_eq!(registry.policies[0].model_class, "Post");
    assert_eq!(registry.policies[0].policy_class, "PostController");

    // Mockable interface
    assert!(registry.is_mockable("CacheContract"));
}

#[test]
fn test_request_scoped_container_hierarchy() {
    let source = r#"
        class AppConfig {
            public string $env = "production";
        }
    "#;

    let module = compile_source(source).expect("Compilation must succeed");
    let mut parent_container = Container::new();
    parent_container.auto_wire(&module.metadata).expect("Auto-wire must succeed");

    let mut vm = VM::new(module, false);

    // Parent container holds singletons
    parent_container.singleton("AppConfig", "AppConfig");
    let _parent_config = parent_container.resolve("AppConfig", &mut vm).expect("Resolution must succeed");

    // Worker creates request-scoped container (inheriting parent)
    let mut request_container = parent_container.create_request_scope();
    let _child_config = request_container.resolve("AppConfig", &mut vm).expect("Child resolution must succeed");

    // In child container, register request-specific object
    let mut req_obj = VmObject::new("Request");
    req_obj.set_property("id", PhpValue::string("req_9876"));
    request_container.instance("Request", PhpValue::object(req_obj));

    assert!(request_container.has("Request"));
    // Parent does not have request-scoped object
    assert!(!parent_container.has("Request"));
}
