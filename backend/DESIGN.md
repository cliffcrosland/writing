# Design

## Documents

A document has a title and a revision log. To get the document content, replay
the revision log from the beginning. The content is expected to be markdown
text.

Operational transformation (OT) algorithms allow documents to be edited by
multiple users in real-time, like Google Docs and Etherpad.

Users can create a graph of interconnected documents easily. They can use the
`[[...]]` link syntax to create new documents on the fly.

When you view a document, you can see snippets of all of the other documents
that link to it.

Users can create and edit documents while offline. Their local edits will be
merged together properly with remote edits once they go back online.

## Collaborative document editing

A client can make two kinds of edits to a document: insert text at a position,
or delete a range of text.

For each document, the server keeps a revision log of all of the edits that
clients have made over time to the document.

Each client keeps track of the revision number on which it has based its local
edits.

A client may make edits while offline. Local edits are stored by the client on
local disk.

Local edits are composed together to shorten the list of edits to send to the
server. For example, many insertions of one character after another in a
contiguous sequence can be composed into a single insertion of a string
containing all of the characters concatenated together.

When a client goes online, it tells the server the revision number on which it
has based its local edits, and it sends the first edit on its list of composed
local edits.

The server analyzes all edits that appear in the revision log that were
submitted after the revision id given by the client. It uses these edits to
transform the edit provided by the client. The transformation simply adjusts
the position of the edit to account for the new content state.

For example, if the client submitted the edit `Insert(0, "World")`, but the
edit `Insert(0, "Hello ")`, appears in the revision log after the client's
revision id, then the client's edit would be transformed to
`Insert(6, "World")`. Assuming the document content was empty before, the
document content would resolve to `"Hello World"` after both edits.

Once the server has transformed the client's edit, it saves the edit to the
revision log, and it saves the new document content. The server tells the
client about all of the edits that appear in the revision log after the
client's revision id so that the client can catch up. The server also tells the
client the id of the new revision just added to the revision log.

The client transforms all of its local edits with respect to the new edits from
the server. The client then repeats the process, submitting the next edit in
its local edits list.

As new edits are sent to the server, the server broadcasts them to all
connected clients.

If all clients stop editing for a few seconds, all clients will eventually
reach the same document content state.

Our goal is for each edit to be processed by the server in less than 50ms.

The server keeps track of the position of each connected client's cursor and
broadcasts cursor updates to clients.

## Databases

For easy horizontal scalability and excellent latency, DynamoDB is used to
store documents and their revision logs.

Redis is used to store ephemeral information, like the position of the cursors
of any clients currently connected to a document. Also, Redis pubsub
functionality is used to broadcast new edits to all connected clients.
