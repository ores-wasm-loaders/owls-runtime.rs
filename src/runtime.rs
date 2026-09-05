use crate::{ByteStore, Error, Host, Result, Runtime, Transport};
use std::sync::atomic::AtomicBool;
use wasmi::{Config, Engine, Instance, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

pub struct NativeRuntime {
    engine: Engine,
    memory_bytes: usize,
    fuel: u64,
}
pub struct RunningModule {
    pub store: Store<StoreLimits>,
    pub instance: Instance,
}
impl NativeRuntime {
    pub fn new(memory_bytes: usize, fuel: u64) -> Result<Self> {
        if memory_bytes == 0 || fuel == 0 {
            return Err(Error::Budget);
        }
        let mut config = Config::default();
        config.consume_fuel(true);
        Ok(Self {
            engine: Engine::new(&config),
            memory_bytes,
            fuel,
        })
    }
    /// WASI, filesystem, network and process imports are absent unless the host adds them.
    pub fn instantiate<T: Transport, S: ByteStore>(
        &self,
        host: &mut Host<T, S>,
        cancelled: &AtomicBool,
        configure: impl FnOnce(&mut Linker<StoreLimits>) -> Result<()>,
    ) -> Result<RunningModule> {
        if host.release().runtime != Runtime::RawWasm {
            return Err(Error::Runtime);
        }
        let id = host.release().entrypoint.clone();
        let bytes = host.bytes(&id, cancelled)?;
        let module =
            Module::new(&self.engine, &bytes[..]).map_err(|e| Error::Wasm(e.to_string()))?;
        let limits = StoreLimitsBuilder::new()
            .memory_size(self.memory_bytes)
            .instances(1)
            .memories(1)
            .tables(1)
            .build();
        let mut store = Store::new(&self.engine, limits);
        store.limiter(|s| s);
        store
            .set_fuel(self.fuel)
            .map_err(|e| Error::Wasm(e.to_string()))?;
        let mut linker = Linker::new(&self.engine);
        configure(&mut linker)?;
        let instance = linker
            .instantiate(&mut store, &module)
            .and_then(|i| i.start(&mut store))
            .map_err(|e| Error::Wasm(e.to_string()))?;
        Ok(RunningModule { store, instance })
    }
}
