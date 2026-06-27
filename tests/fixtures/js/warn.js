// ventouse JS fixtures — declare-before-use / callees-before-callers (P5).
// Function declarations hoist, but the textual "callee above caller" rule still applies for
// human top-down reading; only unavoidable cycles are exempt.

// JS-W-BAD — caller above callee -> 1 forward-reference warning. dirt 0.
function caller() { return callee(); } // forward ref -> warning
function callee() { return 1; }

// JS-W-GOOD — callee above caller -> 0 warnings. dirt 0.
function leaf() { return 1; }
function top() { return leaf(); }

// JS-W-TDZ — `let`/`const` use before declaration (temporal dead zone) -> warning.
function tdz() {
  const v = x; // use before decl -> warning
  let x = 1;
  return v + x;
}
