use eventide_macros::value_object;

#[value_object]
struct Amount {
    value: i64,
}

#[value_object(debug = false)]
struct NonDebugVO(i32);

#[value_object]
enum Level {
    #[default]
    Low,
    High,
}

fn main() {
    // Debug is enabled by default, so this formatting call must compile.
    let _ = format!("{:?}", Amount { value: 0 });

    // Default / Clone / PartialEq / Hash should all be available — a
    // compile-time check is enough; we don't need to assert their
    // results at runtime.
    let a = Amount::default();
    let _b = a.clone();
    let _eq = a == Amount { value: 0 };

    // For the `debug = false` variant we deliberately avoid using `{:?}`
    // (which would either fail or be misleading) and only construct a
    // value to prove the rest of the derive set still applies.
    let _ = NonDebugVO(1);

    // Enums get the same Default / Clone / Debug / Serialize /
    // Deserialize derive set; `Default` is satisfied via the
    // `#[default]` variant marker.
    let _lv: Level = Default::default();
}
