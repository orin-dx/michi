// 0.4 API hygiene: these enums are `#[non_exhaustive]`, so a match outside michi-core needs a wildcard arm.
fn value(v: michi_core::toon::Value) {
    match v {
        michi_core::toon::Value::Str(_) => {}
        michi_core::toon::Value::Int(_) => {}
        michi_core::toon::Value::UInt(_) => {}
        michi_core::toon::Value::Float(_) => {}
        michi_core::toon::Value::Bool(_) => {}
        michi_core::toon::Value::Null => {}
    }
}

fn kv_value(v: michi_core::KvValue) {
    match v {
        michi_core::KvValue::Text(_) => {}
        michi_core::KvValue::Int(_) => {}
        michi_core::KvValue::Float(_, _) => {}
        michi_core::KvValue::Bool(_) => {}
        michi_core::KvValue::Duration(_) => {}
        michi_core::KvValue::Missing => {}
    }
}

fn retry_config_error(e: michi_core::RetryConfigError) {
    match e {
        michi_core::RetryConfigError::MaxDelayIsZero => {}
        michi_core::RetryConfigError::BaseDelayExceedsMaxDelay { .. } => {}
        michi_core::RetryConfigError::JitterFactorOutOfRange { .. } => {}
    }
}

fn main() {
    value(michi_core::toon::Value::Null);
    kv_value(michi_core::KvValue::Missing);
    retry_config_error(michi_core::RetryConfigError::MaxDelayIsZero);
}
