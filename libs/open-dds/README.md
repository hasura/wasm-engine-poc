## Porting open-dds to WASM

### ndc-client is the only breaking dependency for being able to ship on wasm32-unknown-unknown

The only dependencies required from ndc-client are in the models files, which do not have any issues compiling to wasm32

By copying the code from the models.rs as well as the apis.rs file in the ndc-spec sdk and removing the ndc-client dependency for open-dds, this code can compile to wasm.