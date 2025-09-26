use crate::error::Result;
use crate::state::ComponentRunStates;
use wasmtime_wasi::WasiCtxBuilder;

/// Create a default WASI context for component execution
pub fn create_default_wasi_context() -> Result<ComponentRunStates> {
    let wasi_ctx = WasiCtxBuilder::new().inherit_stdio().inherit_args().build();

    Ok(ComponentRunStates {
        wasi_ctx,
        resource_table: wasmtime::component::ResourceTable::new(),
        http_ctx: wasmtime_wasi_http::WasiHttpCtx::new(),
    })
}
