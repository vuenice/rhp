use rhp::{run_source_with_vm, PhpValue};

#[test]
fn test_basic_namespace_declaration_and_instantiation() {
    let source = r#"
        namespace App\Actions\Site;

        class Handler {
            public string $action = 'render_home';
            public function getAction(): string {
                return $this->action;
            }
        }

        $h = new Handler();
        $res1 = $h->getAction();

        $h2 = new \App\Actions\Site\Handler();
        $res2 = $h2->getAction();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Namespace execution should succeed");
    assert_eq!(
        vm.get_main_variable("$res1"),
        Some(&PhpValue::String("render_home".into()))
    );
    assert_eq!(
        vm.get_main_variable("$res2"),
        Some(&PhpValue::String("render_home".into()))
    );
}

#[test]
fn test_use_import_and_aliasing() {
    let source = r#"
        namespace App\Models {
            class User {
                public string $name = 'Alice';
                public function getName(): string {
                    return $this->name;
                }
            }
        }

        namespace App\Actions\Site {
            use App\Models\User;
            use App\Models\User as ModelUser;

            $u1 = new User();
            $n1 = $u1->getName();

            $u2 = new ModelUser();
            $n2 = $u2->getName();

            $u3 = new \App\Models\User();
            $n3 = $u3->getName();
        }
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Use and aliasing execution should succeed");
    assert_eq!(
        vm.get_main_variable("$n1"),
        Some(&PhpValue::String("Alice".into()))
    );
    assert_eq!(
        vm.get_main_variable("$n2"),
        Some(&PhpValue::String("Alice".into()))
    );
    assert_eq!(
        vm.get_main_variable("$n3"),
        Some(&PhpValue::String("Alice".into()))
    );
}

#[test]
fn test_new_exception_with_leading_backslash_and_methods() {
    let source = r#"
        $e = new \Exception("Database connection timeout", 504);
        $msg = $e->getMessage();
        $code = $e->getCode();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("new \\Exception should succeed");
    assert_eq!(
        vm.get_main_variable("$msg"),
        Some(&PhpValue::String("Database connection timeout".into()))
    );
    assert_eq!(vm.get_main_variable("$code"), Some(&PhpValue::Int(504)));
}

#[test]
fn test_try_catch_with_namespaced_and_global_exception() {
    let source = r#"
        namespace App\Services;

        $caughtMsg = "";
        $caughtCode = 0;

        try {
            throw new \Exception("Validation failed", 422);
        } catch (\Exception $e) {
            $caughtMsg = $e->getMessage();
            $caughtCode = $e->getCode();
        }
    "#;

    let (vm, _) = run_source_with_vm(source).expect("try catch with \\Exception should succeed");
    assert_eq!(
        vm.get_main_variable("$caughtMsg"),
        Some(&PhpValue::String("Validation failed".into()))
    );
    assert_eq!(
        vm.get_main_variable("$caughtCode"),
        Some(&PhpValue::Int(422))
    );
}

#[test]
fn test_subnamespace_use_and_instantiation() {
    let source = r#"
        namespace App\Models {
            class User {
                public int $id = 99;
                public function getId(): int {
                    return $this->id;
                }
            }
        }

        namespace App\Http {
            use App\Models;

            $user = new Models\User();
            $userId = $user->getId();
        }
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Sub-namespace use import should succeed");
    assert_eq!(vm.get_main_variable("$userId"), Some(&PhpValue::Int(99)));
}

#[test]
fn test_class_constant_resolution_with_namespaces_and_aliases() {
    let source = r#"
        namespace App\Models {
            class User {
                public const ROLE = 'ADMINISTRATOR';
            }
        }

        namespace App\Actions {
            use App\Models\User as UserModel;

            $c1 = UserModel::ROLE;
            $c2 = \App\Models\User::ROLE;
        }
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Class const resolution with namespaces should succeed");
    assert_eq!(
        vm.get_main_variable("$c1"),
        Some(&PhpValue::String("ADMINISTRATOR".into()))
    );
    assert_eq!(
        vm.get_main_variable("$c2"),
        Some(&PhpValue::String("ADMINISTRATOR".into()))
    );
}

#[test]
fn test_namespaced_interfaces_and_instanceof() {
    let source = r#"
        namespace App\Contracts {
            interface Greeter {
                public function greet(): string;
            }
        }

        namespace App\Services {
            use App\Contracts\Greeter;

            class Service implements Greeter {
                public function greet(): string {
                    return "hello from service";
                }
            }

            $s = new Service();
            $msg = $s->greet();
            $isDirect = $s instanceof Greeter;
            $isFqn = $s instanceof \App\Contracts\Greeter;
        }
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Namespaced interfaces should succeed");
    assert_eq!(
        vm.get_main_variable("$msg"),
        Some(&PhpValue::String("hello from service".into()))
    );
    assert_eq!(vm.get_main_variable("$isDirect"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$isFqn"), Some(&PhpValue::Bool(true)));
}

#[test]
fn test_leading_backslash_facade_calls_inside_namespace() {
    let source = r#"
        namespace App\Actions;

        $upper = \Str::upper("namespaced string");
        $hasKey = \Arr::has(['a' => 1], 'a');
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Leading backslash on facades should succeed");
    assert_eq!(
        vm.get_main_variable("$upper"),
        Some(&PhpValue::String("NAMESPACED STRING".into()))
    );
    assert_eq!(vm.get_main_variable("$hasKey"), Some(&PhpValue::Bool(true)));
}

#[test]
fn test_multiple_use_imports_in_single_statement() {
    let source = r#"
        namespace App\Entities {
            class Post {
                public string $title = 'Post Title';
            }
            class Comment {
                public string $body = 'Comment Body';
            }
        }

        namespace App\Controllers {
            use App\Entities\Post, App\Entities\Comment as PostComment;

            $p = new Post();
            $c = new PostComment();
            $title = $p->title;
            $body = $c->body;
        }
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Multiple use imports in single statement should succeed");
    assert_eq!(
        vm.get_main_variable("$title"),
        Some(&PhpValue::String("Post Title".into()))
    );
    assert_eq!(
        vm.get_main_variable("$body"),
        Some(&PhpValue::String("Comment Body".into()))
    );
}

#[test]
fn test_class_extends_qualified_parent_and_trait_use() {
    let source = r#"
        namespace App\Traits {
            trait Timestampable {
                public string $createdAt = '2026-01-01';
                public function getCreatedAt(): string {
                    return $this->createdAt;
                }
            }
        }

        namespace App\Base {
            class Model {
                public int $id = 100;
                public function getId(): int {
                    return $this->id;
                }
            }
        }

        namespace App\Models {
            class User extends \App\Base\Model {
                use \App\Traits\Timestampable;

                public string $role = 'member';
            }

            $u = new User();
            $id = $u->getId();
            $created = $u->getCreatedAt();
            $role = $u->role;
        }
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Inheritance and trait use with qualified names should succeed");
    assert_eq!(vm.get_main_variable("$id"), Some(&PhpValue::Int(100)));
    assert_eq!(
        vm.get_main_variable("$created"),
        Some(&PhpValue::String("2026-01-01".into()))
    );
    assert_eq!(
        vm.get_main_variable("$role"),
        Some(&PhpValue::String("member".into()))
    );
}

#[test]
fn test_qualified_type_hints_in_methods_and_properties() {
    let source = r#"
        namespace App\DTO {
            class UserData {
                public string $username = 'admin_user';
            }
        }

        namespace App\Services {
            use App\DTO\UserData;

            class UserService {
                public function transform(\App\DTO\UserData $data): string {
                    return $data->username;
                }
            }

            $service = new UserService();
            $data = new UserData();
            $result = $service->transform($data);
        }
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Qualified type hints in method signatures should succeed");
    assert_eq!(
        vm.get_main_variable("$result"),
        Some(&PhpValue::String("admin_user".into()))
    );
}

#[test]
fn test_user_requested_dnf_and_namespaced_union_types() {
    let source = r#"
        interface HasId {}
        interface Arrayable {}

        class IdArrayItem implements HasId, Arrayable {}

        // Accepts: an object implementing BOTH interfaces, OR null
        function save((HasId&Arrayable)|null $item): string {
            if ($item === null) {
                return "saved_null";
            }
            return "saved_item";
        }

        // Mixed: (A and B) or C
        function render((Countable&Traversable)|array $data): string {
            return "rendered";
        }

        // Return and property types work too
        class Repo {
            private (HasId&Arrayable)|null $cached = null;

            public function find(): (HasId&Arrayable)|false {
                return false;
            }

            public function setCache((HasId&Arrayable)|null $val): void {
                $this->cached = $val;
            }

            public function getCache(): (HasId&Arrayable)|null {
                return $this->cached;
            }
        }

        $itemObj = new IdArrayItem();
        $saveRes1 = save(null);
        $saveRes2 = save($itemObj);

        $renderRes1 = render([1, 2, 3]);

        $repo = new Repo();
        $findRes = $repo->find();
        $repo->setCache($itemObj);
        $cachedRes = $repo->getCache();
        $hasCached = ($cachedRes !== null);

        namespace App\Models {
            class User {}
            class Admin {}
            class Team {}
        }

        namespace App\Services {
            use App\Models\User;
            use App\Models\Admin as AdminModel;

            function notify(User|AdminModel|\Stringable $recipient): string {
                return "notified";
            }

            // Fully qualified are not supported, even in return value or function parameter
            function handle(\App\Models\User|\App\Models\Team $subject): \App\Models\User|\App\Models\Team {
                return $subject;
            }

            $u = new User();
            $res1 = notify($u);

            $admin = new AdminModel();
            $resAdmin = notify($admin);

            $t = new \App\Models\Team();
            $res2 = handle($t);
            $isTeam = ($res2 instanceof \App\Models\Team);
        }
    "#;

    let (vm, _) = run_source_with_vm(source).expect("DNF and namespaced union types must succeed");
    assert_eq!(vm.get_main_variable("$saveRes1"), Some(&PhpValue::String("saved_null".into())));
    assert_eq!(vm.get_main_variable("$saveRes2"), Some(&PhpValue::String("saved_item".into())));
    assert_eq!(vm.get_main_variable("$renderRes1"), Some(&PhpValue::String("rendered".into())));
    assert_eq!(vm.get_main_variable("$findRes"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$hasCached"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$res1"), Some(&PhpValue::String("notified".into())));
    assert_eq!(vm.get_main_variable("$resAdmin"), Some(&PhpValue::String("notified".into())));
    assert_eq!(vm.get_main_variable("$isTeam"), Some(&PhpValue::Bool(true)));
}

#[test]
fn test_dnf_type_rejects_partial_interface_implementation_at_compile_time() {
    let source = r#"
        interface HasId {}
        interface Arrayable {}

        class OnlyHasId implements HasId {}

        function save((HasId&Arrayable)|null $item): void {}

        $item = new OnlyHasId();
        save($item);
    "#;

    let res = run_source_with_vm(source);
    assert!(res.is_err(), "Passing object implementing only one interface to DNF intersection must fail");
    let err_str = format!("{:?}", res.err().unwrap());
    assert!(err_str.contains("ArgumentTypeMismatch"), "Expected ArgumentTypeMismatch error, got: {}", err_str);
}

#[test]
fn test_dnf_syntax_errors_for_unparenthesized_mixing() {
    // Unparenthesized & with | should fail parsing
    let source1 = r#"
        function test(A & B | C $item): void {}
    "#;
    let res1 = run_source_with_vm(source1);
    assert!(res1.is_err(), "Mixing unparenthesized '&' and '|' must fail parse");

    let source2 = r#"
        function test(A | B & C $item): void {}
    "#;
    let res2 = run_source_with_vm(source2);
    assert!(res2.is_err(), "Mixing unparenthesized '|' and '&' must fail parse");

    // Parenthesized with only 1 member (A) in union must fail
    let source3 = r#"
        function test((A)|B $item): void {}
    "#;
    let res3 = run_source_with_vm(source3);
    assert!(res3.is_err(), "Single member parenthesized type '(A)' must fail parse");
}

#[test]
fn test_dnf_local_variable_declaration_and_multiple_clauses() {
    let source = r#"
        interface A {}
        interface B {}
        interface C {}
        interface D {}

        class AB implements A, B {}
        class CD implements C, D {}

        function check((A&B)|(C&D) $val): string {
            if ($val instanceof A) {
                return "AB";
            }
            return "CD";
        }

        $ab = new AB();
        $cd = new CD();

        // Typed local variables with DNF types
        (A&B)|null $local1 = $ab;
        (C&D)|null $local2 = null;

        $r1 = check($ab);
        $r2 = check($cd);
        $isLocal1Set = ($local1 !== null);
        $isLocal2Null = ($local2 === null);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Multiple DNF clauses and local variables must succeed");
    assert_eq!(vm.get_main_variable("$r1"), Some(&PhpValue::String("AB".into())));
    assert_eq!(vm.get_main_variable("$r2"), Some(&PhpValue::String("CD".into())));
    assert_eq!(vm.get_main_variable("$isLocal1Set"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$isLocal2Null"), Some(&PhpValue::Bool(true)));
}

#[test]
fn test_namespaced_dnf_types_with_fully_qualified_and_aliased_interfaces() {
    let source = r#"
        namespace App\Contracts {
            interface Identifiable {}
            interface Serializable {}
        }

        namespace App\Domain {
            use App\Contracts\Identifiable;
            use App\Contracts\Serializable as JsonSerializable;

            class Entity implements Identifiable, JsonSerializable {}

            class Handler {
                public function process((Identifiable&JsonSerializable)|null $target): string {
                    if ($target === null) {
                        return "processed_null";
                    }
                    return "processed_entity";
                }

                public function export(): (\App\Contracts\Identifiable&\App\Contracts\Serializable)|false {
                    return new Entity();
                }
            }

            $h = new Handler();
            $e = new Entity();
            $res1 = $h->process($e);
            $res2 = $h->process(null);
            $exported = $h->export();
            $isExportedEntity = ($exported instanceof Entity);
        }
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Namespaced DNF types must succeed");
    assert_eq!(vm.get_main_variable("$res1"), Some(&PhpValue::String("processed_entity".into())));
    assert_eq!(vm.get_main_variable("$res2"), Some(&PhpValue::String("processed_null".into())));
    assert_eq!(vm.get_main_variable("$isExportedEntity"), Some(&PhpValue::Bool(true)));
}



