# rxui-host

The native window and presentation lifecycle for
[RXUI](https://github.com/hxyulin/rxui), on winit and wgpu: surface
configuration, frame scheduling and device-loss recovery.

Most applications use it through the `rxui` crate's `native` feature. See the
[application contract](https://github.com/hxyulin/rxui/blob/main/docs/application.md).
