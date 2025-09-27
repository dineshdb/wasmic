use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::WasiHttpCtx;

pub struct ComponentRunStates {
    // These two are required basically as a standard way to enable the impl of IoView and
    // WasiView.
    // impl of WasiView is required by [`wasmtime_wasi::p2::add_to_linker_sync`]
    pub wasi_ctx: WasiCtx,
    pub resource_table: ResourceTable,
    // HTTP context for WASI HTTP support
    pub http_ctx: WasiHttpCtx,
}

impl ComponentRunStates {
    pub fn new() -> Self {
        let wasi_ctx = WasiCtx::builder().inherit_stdio().inherit_args().build();
        Self {
            wasi_ctx,
            resource_table: ResourceTable::new(),
            http_ctx: WasiHttpCtx::new(),
        }
    }
}

impl WasiView for ComponentRunStates {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.resource_table,
        }
    }
}

impl wasmtime_wasi_http::WasiHttpView for ComponentRunStates {
    fn ctx(&mut self) -> &mut WasiHttpCtx {
        &mut self.http_ctx
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.resource_table
    }
}

impl Default for ComponentRunStates {
    fn default() -> Self {
        Self::new()
    }
}
