# Epoch Cell (Unsound)

Like a `RefCell`, but instead of panicking on recursive `.borrow_mut()`, it invalidates the earlier handles whilst the new borrow is alive, and restores them once it ends.

```rust
let c = EpochCell::new(0u32);
{
    let mut a = c.borrow_mut();
    {
        let mut b = c.borrow_mut();
        *b = 2;
    }
    *a = 1;
}
assert_eq!(c.into_inner(), 1);
```


It sounds like UB, and it might be, but I think this is what Stacked Borrows means?
The tests pass miri.

Note that this allows unsoundness:
```
let c = EpochCell::new(0u32);
{
    let mut a = c.borrow_mut();
    let am = &mut*a;
    {
        let mut b = c.borrow_mut();
        let bm = &mut*b;
        *bm = 2;
    }
    *am = 1;
}
assert_eq!(c.into_inner(), 1);
```
