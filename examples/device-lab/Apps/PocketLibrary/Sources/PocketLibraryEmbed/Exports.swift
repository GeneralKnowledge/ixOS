// What Device Lab imports: this app's views and bindings. SwiftPM applies
// Device Lab's moduleAliases only to the targets a product depends on, not
// to the ones it contains, so the product holds only this target and the
// app's own modules (ForgeFFI, AppHost) are renamed under it.
@_exported import AppHost
@_exported import ForgeFFI
