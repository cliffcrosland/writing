# Writing App (Name TBD)

This is a project management app focused on writing.

A six-page memo is often better at conveying information than a slide deck.
When you write a memo, you are forced to write out your ideas in complete
sentences and paragraphs. This helps you think through your ideas more
rigorously and improve your arguments.

With this app, teams collaborate with one another by writing in complete
sentences and paragraphs.

You can easily create many pages containing personal notes. These pages are
linked to one another using bi-directional links.

You can share a subset of these pages with your team and collaborate on them
together in real time.

If you would like, these notes can become documentation for the whole company
to share. Team members can explore related concepts at your company by
following bi-directional links.

If you want to turn a page into a task, you can add the page to a project
board. The task makes progress through the board by moving from column to
column.

When you work on a task, you can open up the task's page, read details written
there, and follow bi-directional links to get acquainted with relevant
documentation and related concepts.


## Todos
- Frontend: Resolve CORS-related security error when local JS app tries to make
  API requests to local backend.
- Frontend: As a user, I want to be able to view a list of documents that I am
  allowed to view or edit.
- Frontend: As a user, I want to be able to create a new document and start to
  edit it.
- Frontend/Backend: As a user, I want my edits to be saved to the backend.
- Frontend/Backend/OT: As a user, I want to be able to use Ctrl-Z and
  Ctrl-Shift-Z to undo/redo local changes and have them compose properly with
  remote changes that have happened in the meantime. Probably need to tweak
  ot::transform, or maybe create a new function, possibly named ot:transpose.
- Frontend: As a user, I want my local edits to be saved to local storage for
  offline mode.
- Choose a good name for this project.
