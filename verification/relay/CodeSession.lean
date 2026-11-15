/-
  Relay: the sender's code session, as a state machine.

  Models `Session` in `examples/relay/spike/src/session.rs` (M0), which the
  sender loop in `main.rs` drives. One code is live at a time; it
  is replaced ("reissued") after a wrong guess, a collision, a dropped
  transfer or expiry. Properties, for every sequence of events:

  1. A code allows at most one guess (one admitted handshake).
  2. At most one receiver is ever admitted on a code.
  3. "busy" and "gone" answers never change the session.
  4. A spent or expired code stays unusable until a new code replaces it.

  Plain Lean 4, no Mathlib: `lean CodeSession.lean` checks it.
-/

structure Session where
  gen      : Nat    -- which code is live (bumped on reissue)
  spent    : Bool   -- a handshake was admitted on this code
  busy     : Bool   -- a handshake is in progress
  expired  : Bool   -- the code outlived its lifetime
  admitted : Nat    -- handshakes admitted on this code
  allowedThisGen : Nat -- receivers allowed to fetch because of this code
  deriving Repr

inductive Event where
  | attempt              -- a receiver sends its first SPAKE2 message
  | finish (g : Nat) (right : Bool) -- the handshake admitted on code `g` ends:
                                    -- key confirmed or not
  | expire
  | reissue              -- wrong guess, collision, dropped transfer: new code
  deriving Repr

/-- What the sender answers to an attempt. -/
inductive Answer where
  | go | busy | gone
  deriving Repr, DecidableEq

def answer (s : Session) : Answer :=
  if s.busy then .busy
  else if s.spent || s.expired then .gone
  else .go

def step (s : Session) : Event → Session
  | .attempt =>
    match answer s with
    | .go   => { s with busy := true, spent := true, admitted := s.admitted + 1 }
    | _     => s
  | .finish g right =>
    if g = s.gen ∧ s.busy = true then
      { s with busy := false,
               allowedThisGen := if right then s.allowedThisGen + 1 else s.allowedThisGen }
    else s
  | .expire => { s with expired := true }
  | .reissue =>
    { gen := s.gen + 1, spent := false, busy := false, expired := false,
      admitted := 0, allowedThisGen := 0 }

def init : Session :=
  { gen := 0, spent := false, busy := false, expired := false,
    admitted := 0, allowedThisGen := 0 }

/-- The invariant: properties 1 and 2, plus what makes them inductive. -/
def SessionInv (s : Session) : Prop :=
  s.admitted ≤ 1 ∧
  s.allowedThisGen ≤ s.admitted ∧
  (s.admitted = 0 ↔ s.spent = false) ∧
  (s.busy = true → s.spent = true ∧ s.allowedThisGen = 0)

theorem inv_init : SessionInv init := by
  simp [SessionInv, init]

theorem inv_step (s : Session) (e : Event) (h : SessionInv s) : SessionInv (step s e) := by
  obtain ⟨h1, h2, h3, h4⟩ := h
  cases e with
  | attempt =>
    unfold step answer
    cases hb : s.busy <;> cases hs : s.spent <;> cases hx : s.expired <;>
      simp_all [SessionInv] <;> omega
  | finish g right =>
    unfold step
    by_cases hg : g = s.gen <;>
      cases hb : s.busy <;> cases right <;> simp_all [SessionInv] <;> omega
  | expire =>
    simp_all [step, SessionInv]
  | reissue =>
    simp [step, SessionInv]

/-- Reachable states: from `init` by any list of events. -/
def run (s : Session) : List Event → Session
  | [] => s
  | e :: es => run (step s e) es

theorem inv_run (s : Session) (es : List Event) (h : SessionInv s) : SessionInv (run s es) := by
  induction es generalizing s with
  | nil => simpa [run]
  | cons e es ih => exact ih _ (inv_step s e h)

/-- Property 1: at most one guess per code, after any events. -/
theorem one_guess_per_code (es : List Event) : (run init es).admitted ≤ 1 :=
  (inv_run init es inv_init).1

/-- Property 2: at most one receiver allowed per code. -/
theorem one_receiver_per_code (es : List Event) : (run init es).allowedThisGen ≤ 1 := by
  have h := inv_run init es inv_init
  exact Nat.le_trans h.2.1 h.1

/-- Property 3: refusals change nothing. -/
theorem refusal_changes_nothing (s : Session) (h : answer s ≠ .go) :
    step s .attempt = s := by
  unfold step
  cases hs : answer s <;> simp_all

/-- Property 4: a spent or expired code is refused until a reissue. -/
theorem spent_stays_refused (s : Session) (h : s.spent = true ∨ s.expired = true) :
    answer s ≠ .go := by
  unfold answer
  cases hb : s.busy <;> cases hs : s.spent <;> cases hx : s.expired <;> simp_all
