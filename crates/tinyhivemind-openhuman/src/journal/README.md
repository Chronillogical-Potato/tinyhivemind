# `journal`

`MemoryLog`: an append-only in-memory journal for one desk that is a real
`SessionLog`. Two rules decide who may read a row, and they are the rules a
host's own journal follows: a desk row with `only_for` reaches its author
and that one seat; a row in a conversation reaches the conversation's two
seats. `append` returns the sequence a row was given; `desk_since`,
`thread` and `thread_since` render rows for a reader. Always compiled: the
example, the tests and the crate's doc example are hosts over it.
