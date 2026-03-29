[![CI](https://github.com/dbongartz/badewanne/actions/workflows/ci.yml/badge.svg)](https://github.com/dbongartz/badewanne/actions/workflows/ci.yml)

# Badewanne

A `no_std`, lock-free, fixed-size object pool.

`Badewanne` [German for *bathtub*] pre-allocates `SIZE` slots.

Values are placed into the pool via `Duck::new_in`, which returns a smart
pointer that dereferences to `T` and returns the slot when dropped.

## Usage

```rust
use badewanne::{Badewanne, Duck};

let pool = Badewanne::<String, 2>::new();

let a = Duck::new_in("hello".into(), &pool).expect("pool has space");
let b = Duck::new_in("world".into(), &pool).expect("pool has space");

// Pool is full.
assert!(Duck::new_in("!".into(), &pool).is_none());

// Dropping a duck frees its slot.
drop(a);
let c = Duck::new_in("back".into(), &pool).expect("slot freed");
assert_eq!(&*c, "back");
```

## Properties

- `no_std` + `no-alloc`.
- Lock-free and thread safe.
