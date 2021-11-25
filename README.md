# Writing App (Name TBD)

This is a side-project I built for fun to learn about collaborative text
editing. It uses the variant of operational transformation that is used in
Google Docs: Clients send one change set at a time.

The project uses Protobufs in its OT protocol, and the collaborative text
editing client is built in Rust with WebAssembly. A document is represented as
a log of change sets. Clients race to append the next change to the log. The
client that loses receives the newly comitted remote changes and transforms its
local changes against them, then retries submission.

The `ot` crate contains the operational transformation primitives that can be
used by the backend and frontend.

The `frontend/wasm` crate contains the WebAssembly OT client that runs in the
browser. It communicates with the backend using an OT protocol implemented with
Protobufs.

The `backend` crate contains the server code that receives Protobuf requests
and handles the OT protocol.
