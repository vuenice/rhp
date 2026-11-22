use rhp::{run_source, PhpValue};

fn string_result(source: &str) -> String {
    let value = run_source(source).expect("program should run");
    match value {
        PhpValue::String(s) => s.to_string(),
        other => panic!("expected string, got {other:?}"),
    }
}

#[test]
fn serialize_scalars_lists_and_dicts() {
    let source = r#"
        return [
            serialize(null),
            serialize(true),
            serialize(7),
            serialize(1.5),
            serialize("hi"),
            serialize([1, 2]),
            serialize(["host" => "localhost", "port" => 80]),
        ];
    "#;
    let value = run_source(source).expect("scalar serialize");
    assert_eq!(
        value,
        PhpValue::list(vec![
            PhpValue::string("null"),
            PhpValue::string("true"),
            PhpValue::string("7"),
            PhpValue::string("1.5"),
            PhpValue::string("\"hi\""),
            PhpValue::string("[1,2]"),
            PhpValue::string("{\"host\":\"localhost\",\"port\":80}"),
        ])
    );
}

#[test]
fn serialize_constructor_envelope_and_roundtrip() {
    let source = r#"
        class Order {
            public function __construct(
                public readonly $id: int,
                private $amount: float
            ) {}

            public function total(): float {
                return $this->amount;
            }
        }

        $order = new Order(42, 99.5);
        $payload = serialize($order);
        $again = unserialize($payload);
        return [$payload, $again->id, $again->total()];
    "#;
    let value = run_source(source).expect("object roundtrip");
    assert_eq!(
        value,
        PhpValue::list(vec![
            PhpValue::string("{\"__class\":\"Order\",\"__data\":{\"id\":42,\"amount\":99.5}}"),
            PhpValue::Int(42),
            PhpValue::Float(99.5),
        ])
    );
}

#[test]
fn missing_keys_use_defaults_and_unknown_keys_are_ignored() {
    let source = r#"
        class Job {
            public function __construct(
                public $name: string,
                public $retry: bool = false,
                public $priority: string = "normal"
            ) {}
        }

        $old = '{"__class":"Job","__data":{"name":"ssl","retry":false,"extra":1}}';
        $job = unserialize($old);
        $newer = unserialize('{"__class":"Job","__data":{"name":"ssl","priority":"high","extra":9}}');
        return [$job->name, $job->retry, $job->priority, $newer->retry, $newer->priority];
    "#;
    let value = run_source(source).expect("schema evolution");
    assert_eq!(
        value,
        PhpValue::list(vec![
            PhpValue::string("ssl"),
            PhpValue::Bool(false),
            PhpValue::string("normal"),
            PhpValue::Bool(false),
            PhpValue::string("high"),
        ])
    );
}

#[test]
fn casts_scalars_to_constructor_types() {
    let source = r#"
        class Sample {
            public function __construct(
                public $n: int,
                public $label: string,
                public $ok: bool
            ) {}
        }

        $raw = '{"__class":"Sample","__data":{"n":"12","label":7,"ok":1}}';
        $sample = unserialize($raw);
        return [$sample->n, $sample->label, $sample->ok];
    "#;
    let value = run_source(source).expect("scalar cast");
    assert_eq!(
        value,
        PhpValue::list(vec![
            PhpValue::Int(12),
            PhpValue::string("7"),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn rejects_out_of_range_int_cast() {
    let source = r#"
        class Sample {
            public function __construct(public $n: int) {}
        }
        unserialize('{"__class":"Sample","__data":{"n":1.5}}');
    "#;
    let err = run_source(source).expect_err("1.5 is not an int");
    let text = err.to_string();
    assert!(text.contains("Cannot cast"), "{text}");
}

#[test]
fn date_tag_roundtrip() {
    let source = r#"
        class Reminder {
            public function __construct(public $at: mixed = null) {}
        }

        $job = new Reminder(Time::parse("2026-11-22T08:56:00Z"));
        $payload = serialize($job);
        $again = unserialize($payload);
        return [$payload, $again->at->timestamp];
    "#;
    let value = run_source(source).expect("date roundtrip");
    match value {
        PhpValue::List(items) | PhpValue::Array(items) => {
            let items = items.borrow();
            let payload = items[0].to_php_string();
            assert!(
                payload.contains("\"_d\":\"2026-11-22T08:56:00Z\""),
                "{payload}"
            );
            assert_eq!(items[1], PhpValue::Int(1_795_337_760));
        }
        other => panic!("expected list, got {other:?}"),
    }
}

#[test]
fn model_serialize_hook_and_find_or_fail() {
    let source = r#"
        class Site {
            public function __construct(public $id: int, public $name: string) {}

            public function __serialize(): mixed {
                return ["_m" => "Site", "id" => $this->id, "connection" => "mysql"];
            }

            public static function findOrFail($id: int): Site {
                return new Site($id, "db");
            }
        }

        class CheckSslStatus {
            public function __construct(
                public $site: Site,
                public $isRetry: bool = false
            ) {}
        }

        $job = new CheckSslStatus(new Site(1042, "live"), false);
        $payload = serialize($job);
        $again = unserialize($payload);
        return [$payload, $again->site->id, $again->site->name, $again->isRetry];
    "#;
    let value = run_source(source).expect("model hook");
    assert_eq!(
        value,
        PhpValue::list(vec![
            PhpValue::string(
                "{\"__class\":\"CheckSslStatus\",\"__data\":{\"site\":{\"_m\":\"Site\",\"id\":1042,\"connection\":\"mysql\"},\"isRetry\":false}}"
            ),
            PhpValue::Int(1042),
            PhpValue::string("db"),
            PhpValue::Bool(false),
        ])
    );
}

#[test]
fn model_unserialize_hook_when_find_or_fail_is_absent() {
    let source = r#"
        class Note {
            public $id: int = 0;

            public function __serialize(): mixed {
                return ["_m" => "Note", "id" => 7];
            }

            public function __unserialize($data: dict): void {
                $this->id = $data["id"];
            }
        }

        class Wrap {
            public function __construct(public $note: Note) {}
        }

        $wrap = unserialize(serialize(new Wrap(new Note())));
        return $wrap->note->id;
    "#;
    let value = run_source(source).expect("unserialize hook");
    assert_eq!(value, PhpValue::Int(7));
}

#[test]
fn model_collection_calls_where_in_get() {
    let source = r#"
        class IdQuery {
            public function __construct(public $ids: list) {}
            public function get(): list {
                return $this->ids;
            }
        }

        class Site {
            public static function whereIn($column: string, $ids: list): IdQuery {
                return new IdQuery($ids);
            }
        }

        class Bag {
            public function __construct(public $sites: list) {}
        }

        $bag = unserialize('{"__class":"Bag","__data":{"sites":{"_m_coll":"Site","ids":[1,2]}}}');
        return $bag->sites;
    "#;
    let value = run_source(source).expect("model collection");
    assert_eq!(
        value,
        PhpValue::list(vec![PhpValue::Int(1), PhpValue::Int(2)])
    );
}

#[test]
fn injected_service_is_omitted_and_optional_service_stays_default() {
    let source = r#"
        class Logger {
            public function __construct(public $name: string) {}
        }

        class Job {
            public function __construct(
                public $label: string,
                $logger: Logger? = null
            ) {}
        }

        $payload = serialize(new Job("ssl", new Logger("disk")));
        $again = unserialize($payload);
        return [$payload, $again->label];
    "#;
    let value = run_source(source).expect("service omitted");
    assert_eq!(
        value,
        PhpValue::list(vec![
            PhpValue::string("{\"__class\":\"Job\",\"__data\":{\"label\":\"ssl\"}}"),
            PhpValue::string("ssl"),
        ])
    );
}

#[test]
fn nested_objects_lists_and_namespace() {
    let source = r#"
        namespace App\Jobs;

        class Point {
            public function __construct(public $x: int, public $y: int) {}
        }

        class Line {
            public function __construct(public $start: Point, public $points: list) {}
        }

        $line = new Line(new Point(1, 2), [new Point(3, 4)]);
        $again = unserialize(serialize($line));
        return [$again->start->x, $again->start->y, $again->points[0]->y];
    "#;
    let value = run_source(source).expect("nested objects");
    assert_eq!(
        value,
        PhpValue::list(vec![PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(4)])
    );

    let payload = string_result(
        r#"
        namespace App\Jobs;
        class Ping {
            public function __construct(public $ok: bool = true) {}
        }
        return serialize(new Ping(true));
        "#,
    );
    assert!(
        payload.contains(r#""__class":"App\\Jobs\\Ping""#),
        "{payload}"
    );
}

#[test]
fn closure_resource_cycle_and_bad_payload_are_rejected() {
    let closure = run_source(
        r#"
        class Box {
            public function __construct(public $fn: mixed) {}
        }
        return serialize(new Box(fn(): int => 1));
        "#,
    )
    .expect_err("closure");
    assert!(closure.to_string().contains("Cannot serialize closure"), "{closure}");

    let cycle = run_source(
        r#"
        class Node {
            public $next: mixed = null;
        }
        $a = new Node();
        $a->next = $a;
        return serialize($a);
        "#,
    )
    .expect_err("cycle");
    assert!(
        cycle.to_string().contains("circular reference"),
        "{cycle}"
    );

    let bad = run_source(r#"return unserialize("{");"#).expect_err("json");
    assert!(bad.to_string().contains("Invalid serialized payload"), "{bad}");

    let missing = run_source(
        r#"
        class Job {
            public function __construct(public $name: string) {}
        }
        return unserialize('{"__class":"Job","__data":{}}');
        "#,
    )
    .expect_err("missing field");
    assert!(missing.to_string().contains("Missing required parameter $name"), "{missing}");
}

#[test]
fn enum_roundtrip() {
    let source = r#"
        enum OrderStatus: string {
            case Pending = "pending";
            case Paid = "paid";
        }

        class Ticket {
            public function __construct(public $status: mixed) {}
        }

        $ticket = unserialize(serialize(new Ticket(OrderStatus::Paid)));
        return [$ticket->status->name, $ticket->status->value];
    "#;
    let value = run_source(source).expect("enum roundtrip");
    assert_eq!(
        value,
        PhpValue::list(vec![
            PhpValue::string("Paid"),
            PhpValue::string("paid"),
        ])
    );
}
