# rxui-draw

The GPU drawing layer of [RXUI](https://github.com/hxyulin/rxui): shapes, paths,
images, shadows and shaped text on wgpu. It records into an application-owned
device, queue, encoder and target views.

Most applications use it through the `rxui` crate's `rendering` feature. See
the [wgpu embedding contract](https://github.com/hxyulin/rxui/blob/main/docs/wgpu-embedding.md).
