use rhp::{run_source, PhpValue};

#[test]
fn encode_decode_match_php_sugar() {
    let source = r#"
        $value = ["sku" => "LAPTOP", "qty" => 1, "tags" => ["a", "b"]];
        $encoded = Json::encode($value);
        $decoded = Json::decode($encoded);
        return [
            $encoded,
            $encoded == json_encode($value),
            $decoded == json_decode($encoded),
            Json::stringify($value) == $encoded,
        ];
    "#;
    let value = run_source(source).expect("json encode");
    assert_eq!(
        value,
        PhpValue::list(vec![
            PhpValue::string("{\"sku\":\"LAPTOP\",\"qty\":1,\"tags\":[\"a\",\"b\"]}"),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn parse_returns_dict_and_rejects_a_json_array() {
    let source = r#"
        $row: dict = Json::parse('{"host":"localhost","port":80}');
        return [$row["host"], $row["port"]];
    "#;
    let value = run_source(source).expect("json parse");
    assert_eq!(
        value,
        PhpValue::list(vec![
            PhpValue::string("localhost"),
            PhpValue::Int(80),
        ])
    );

    let err = run_source(r#"return Json::parse("[1,2]");"#).expect_err("array is not a dict");
    let message = err.to_string();
    assert!(
        message.contains("Json::parse() expects a JSON object"),
        "{message}"
    );
}

#[test]
fn serialize_and_deserialize_roundtrip_a_class() {
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
        $payload = Json::serialize($order);
        $again = Json::deserialize($payload);
        return [$payload, $again->id, $again->total()];
    "#;
    let value = run_source(source).expect("class serialize");
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
fn serialize_rejects_non_objects() {
    let err = run_source(r#"return Json::serialize(["id" => 1]);"#).expect_err("dict is not a class");
    let message = err.to_string();
    assert!(
        message.contains("Json::serialize() expects a class object"),
        "{message}"
    );

    let err = run_source(
        r#"return Json::deserialize('{"host":"localhost"}');"#,
    )
    .expect_err("plain object is not a class envelope");
    let message = err.to_string();
    assert!(
        message.contains("Json::deserialize() expects a serialized class object"),
        "{message}"
    );
}

#[test]
fn encode_options_dict() {
    let source = r#"
        return [
            Json::encode(["a" => 1], ["pretty" => true]),
            Json::encode([1, 2], ["forceObject" => true]),
            Json::encode("<", ["html" => true]),
            Json::encode(1.0, ["preserveZeroFraction" => true]),
            Json::stringify(["n" => "12"], ["numericCheck" => true]),
        ];
    "#;
    let value = run_source(source).expect("encode options");
    assert_eq!(
        value,
        PhpValue::list(vec![
            PhpValue::string("{\n    \"a\": 1\n}"),
            PhpValue::string("{\"0\":1,\"1\":2}"),
            PhpValue::string("\"\\u003C\""),
            PhpValue::string("1.0"),
            PhpValue::string("{\"n\":12}"),
        ])
    );
}

#[test]
fn encode_rejects_bad_options_and_validate_checks_text() {
    let err = run_source(r#"return Json::encode(1, ["nope" => true]);"#).expect_err("unknown option");
    assert!(err.to_string().contains("unknown option 'nope'"), "{}", err);

    let err = run_source(r#"return Json::encode(1, 128);"#).expect_err("flags int");
    assert!(err.to_string().contains("options must be a dict"), "{}", err);

    let value = run_source(
        r#"return [Json::validate('{"a":1}'), Json::validate('['), Json::validate('')];"#,
    )
    .expect("validate");
    assert_eq!(
        value,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(false),
        ])
    );
}
