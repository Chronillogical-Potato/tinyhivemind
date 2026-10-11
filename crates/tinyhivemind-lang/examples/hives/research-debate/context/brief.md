# Embedding index brief

<!-- item:latency -->
Rebuilding the index takes 41 seconds on the reference corpus; loading a cached
copy takes 1.2 seconds.

<!-- item:staleness -->
The corpus changes on roughly one run in five. A cache keyed on the corpus
digest is invalidated exactly then.

<!-- item:prior-work -->
The 2025 retrieval study cached its index and reported no quality change, on a
static corpus only.
