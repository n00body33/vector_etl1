use super::*;
use crate::config::log_schema;
use bytes::BytesMut;
use pretty_assertions::assert_eq;
use quickcheck::{QuickCheck, TestResult};
use regex::Regex;

#[test]
fn back_and_forth_through_bytes() {
    fn inner(event: Event) -> TestResult {
        let expected = event.clone();

        let mut buffer = BytesMut::with_capacity(64);
        {
            let res = Event::encode(event, &mut buffer);
            assert!(res.is_ok());
        }
        {
            let res = Event::decode(buffer);
            assert!(res.is_ok());
            assert_eq!(expected, res.unwrap());
        }
        TestResult::passed()
    }

    QuickCheck::new()
        .tests(100)
        .max_tests(1000)
        .quickcheck(inner as fn(Event) -> TestResult);
}

#[test]
fn serialization() {
    let mut event = Event::from("raw log line");
    event.as_mut_log().insert("foo", "bar");
    event.as_mut_log().insert("bar", "baz");

    let expected_all = serde_json::json!({
        "message": "raw log line",
        "foo": "bar",
        "bar": "baz",
        "timestamp": event.as_log().get(log_schema().timestamp_key()),
    });

    let actual_all = serde_json::to_value(event.as_log().all_fields()).unwrap();
    assert_eq!(expected_all, actual_all);

    let rfc3339_re = Regex::new(r"\A\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z\z").unwrap();
    assert!(rfc3339_re.is_match(actual_all.pointer("/timestamp").unwrap().as_str().unwrap()));
}

#[test]
fn type_serialization() {
    use serde_json::json;

    let mut event = Event::from("hello world");
    event.as_mut_log().insert("int", 4);
    event.as_mut_log().insert("float", 5.5);
    event.as_mut_log().insert("bool", true);
    event.as_mut_log().insert("string", "thisisastring");

    let map = serde_json::to_value(event.as_log().all_fields()).unwrap();
    assert_eq!(map["float"], json!(5.5));
    assert_eq!(map["int"], json!(4));
    assert_eq!(map["bool"], json!(true));
    assert_eq!(map["string"], json!("thisisastring"));
}
