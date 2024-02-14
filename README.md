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