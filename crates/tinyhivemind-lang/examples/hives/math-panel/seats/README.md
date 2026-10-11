# Seat prompts

One prompt per seat. A seat names its role in `hive.json`. Each prompt opens
with a persona (`You are a "<persona>".`) and a sentence on how it thinks,
which a `--live` model plays; the offline simulation ignores it.

| File | Role |
| --- | --- |
| `conjecturer.md` | conjecturer |
| `solver-a.md` | solver |
| `solver-b.md` | solver |
| `checker.md` | checker |
| `lead.md` | lead |
