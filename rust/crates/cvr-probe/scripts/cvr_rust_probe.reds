// Stage 1 probe (rust/crates/cvr-probe): the script side of the Rust native. The main menu calls it twice, the
// second time with the first answer, so the log shows 41 -> 42 and 42 -> 43 when arguments and results both cross.
native func CVRRustProbe(value: Int32) -> Int32;

@wrapMethod(SingleplayerMenuGameController)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  let first: Int32 = CVRRustProbe(41);
  CVRRustProbe(first);
  return result;
}
