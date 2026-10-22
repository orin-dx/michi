// These types are never serialized, so a JSON Schema for them would describe a wire form that does not exist.
fn needs_schema<T: schemars::JsonSchema>() {}

fn main() {
    needs_schema::<michi_core::StatusResponse>();
    needs_schema::<michi_core::StatusItem>();
    needs_schema::<michi_core::Health>();
    needs_schema::<michi_core::toon::ToonOptions>();
    needs_schema::<michi_core::ErrorClass>();
    needs_schema::<michi_core::KvItem>();
    needs_schema::<michi_core::OutputFormat>();
    needs_schema::<michi_core::pipeline::Pipeline>();
    needs_schema::<michi_core::pipeline::PipelineStep>();
}
