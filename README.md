# `servt`
> a programmatic web server, with very few dependencies.
## why
- many big frameworks are too heavy for small projects, or don't suit yours (or my!) development style
- so this is small, easy to use, but still powerful
## features
### `async` 
- enabled by default, adds `smol` as a dependency (adds around 35 deps)
- executes all requests by spawning a new task
- if not enabled, falls back to `std::thread` (which spawns a new thread for each request)
### `time`
- enabled by default, adds `chrono` as a dependency (adds around 2 deps, as the features enabled are very few, namely `alloc` and `now`)
- adds a `Date` header to all responses
- this is technically required by the HTTP/1.1 spec, but the majority of clients will work without it