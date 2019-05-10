# Notes from review

* WebKit's documentation and intro on parking lot and its paradigm is quite good

* `cargo test` fails with compilation errors on Linux
  * had an ancient lock file on my system and `cargo update` was needed

* Various bits and pieces of the library are centered around compatibility with
  various versions of Rust. This naturally isn't needed when integrating into
  libstd, but it can often reduce the readability of code and/or make it more
  difficult to follow. Do we want to have a clear version policy for parking_lot
  on crates.io which is geared towards simplifying the code?

* Overall quite worried about atomic orderings. Everything should be `SeqCst`
  until proven otherwise, and even then needs quite a lot of proof (IMO) to not
  use `SeqCst`. Being fast-and-loose with orderings seems like we're trying to
  be too clever by half.

* The `ThreadParker` API seems pretty subtle. For example the `UnparkHandle`
  type can outlive the `ThreadParker` itself but that's ok. This seems currently
  undocumented?

* Can `parking_lot::Mutex` be used in as many places as the current OS-based
  mutex? I think so but it's worth considering. For example you may be able to
  use `std::sync::Mutex` today to protect a global allocator, but with parking
  lot you won't be able to. Similarly about thread local internals with libstd,
  but I think it's just internal stuff to libstd.

* Where possible it'd be best to use `alloc`-the-crate now that it's stable
  instead of using collections through `std`. Ideally only thread locals are
  used from std (and other synchronization things if necessary).

* There is a **massive** number of `#[inline]` annotations everywhere. I suspect
  almost all of them are not necessary and can probably be removed. A
  particularly egregious offender, for example, is `parking_lot_core::park`.
  It's already generic and therefore inlined across crates, and it's a pretty
  massive function to hint even more strongly that it should be inlined.

* In general there's very little documentation beyond the bare bones required by
  the `missing_docs` lint. Even that documentation isn't really anything beyond
  what's already given in the function name and signature. Could benefit from
  quite a few more comments about implementation strategies, protocols
  implemented, meanings of constants, etc. More high level docs would also be
  quite welcome explaining what each module is doing and how it relates to the
  WTF parking lot.

* It seems that `parking_lot_core` is fundamentally unsafe in a way that can't
  really be library-ified? Given that `parking_lot`'s functions are `unsafe` due
  to ensuring that the tokens aren't reused, it basically requires that
  literally everything using the interface agrees on what tokens to allocate?
  Using pointers works, but it seems like it's worthwhile documenting that it's
  basically very difficult to use `parking_lot_core` in a safe fashion unless
  you're doing the exact same thing as everyone else.

* There's virtually no tests from what I can tell other than weak smoke tests
  from the standard library. The `parking_lot_core` crate, for example, seems to
  have virtually no tests of its APIs.

* `Condvar` still can only be used with one mutex at a time (originally thought
  this was going to be fixed)
