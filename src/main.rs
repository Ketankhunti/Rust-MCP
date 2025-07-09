fn main() {
    use serde_json::Value;

let json_str = r#"{"foo": 42, "bar": [1, 2, 3]}"#;
let val: Value = serde_json::from_str(json_str).unwrap();
println!("{:?}", val);
}
