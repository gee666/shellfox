use serde_json::Value;

/// Compare values and field presence, ignoring object key order and numeric spelling.
pub fn assert_same(actual: &Value, expected: &Value) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => assert_eq!(a.as_f64(), b.as_f64()),
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(
                a.keys().collect::<std::collections::BTreeSet<_>>(),
                b.keys().collect::<std::collections::BTreeSet<_>>(),
                "JSON field presence changed"
            );
            for (key, value) in a {
                assert_same(value, &b[key]);
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                assert_same(a, b);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}
