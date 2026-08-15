# Stream run updates over SSE

The Rust service streams live Run status and output to the React dashboard with Server-Sent Events. The React client consumes that stream through authenticated `fetch` rather than native `EventSource`, because the shared bearer token must remain in an Authorization header rather than appear in a URL. Updates travel only from server to browser, and SSE supplies simple reconnection semantics without the bidirectional protocol and lifecycle complexity of WebSockets.
