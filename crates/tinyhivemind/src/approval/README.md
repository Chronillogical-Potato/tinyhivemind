# Approval runtime module

The pure core decides allow, deny, or ask. This module owns the sole waiting
boundary: one host transaction that revalidates and records a human question at
most once. It validates that a returned grant or refusal matches the scope the
pure decision offered.
