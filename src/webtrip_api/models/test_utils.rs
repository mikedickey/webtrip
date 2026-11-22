use serde::{Deserialize, Serialize};

pub fn roundtrip<T>(v: &T) -> String
where
    T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let s = serde_json::to_string(v).expect("serialize");
    let back: T = serde_json::from_str(&s).expect("deserialize");
    assert_eq!(v, &back);
    s
}
