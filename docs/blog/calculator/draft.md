# A Few Buttons, Infinite Suffering

*Or: how rebuilding a calculator became an exercise in digital archaeology, accessibility-tree archaeology, and questioning my life choices.*

<!-- Kept in step with what actually happened: see log.md next to this file. "1. Survey the terrain" and "2. Collect evidence" happened on 2026-11-18 and are written from the field notes; everything after them is still the plan. -->

I thought rebuilding a calculator would be easy.

There are buttons. There are numbers. There are some operators. You press things, numbers appear, and eventually you get an answer.

How complicated could it possibly be?

As it turns out, a calculator is a deceptively small concentration of suffering.

The idea started simple: take an existing iOS app, inspect it from the outside, understand how it works, and manually recreate it. No source code. No peeking behind the curtain. Just the app itself, its accessibility tree, screenshots, and a collection of scripts to help investigate it.

But the more interesting question turned out not to be "can we rebuild it?" It's this:

**What can you learn about a piece of software simply by exploring everything it allows you to observe?**

So this isn't really a rebuild. It's an expedition. We don't know how the thing works inside, so we survey the remains, collect evidence, form hypotheses, and gradually construct a picture of the system. The calculator is our first specimen. The scripts and tools are our expedition equipment. The accessibility tree is one of our maps. The recreation is our attempt to prove we understood what we found.

## The expedition notebook

Every finding in this article gets one of three labels:

- **Observed:** something we directly captured. A tree, a screenshot, a recorded result.
- **Inferred:** something we believe based on the evidence.
- **Unknown:** something we haven't figured out yet.

It sounds pedantic. It is pedantic. It's also the only way I've found to stop "I'm fairly sure it does this" quietly becoming "it does this" somewhere around the third coffee.

The notebook also keeps our failures. If a button has no useful label, or the display refuses to tell us its number, that isn't a failed expedition. It's a finding: Observed, the calculator wouldn't say. It goes in the notebook, kept well apart from whatever we guess to get round it.

## An app shows you several faces

Most people see an application as a visual interface. But an app exposes several overlapping representations of itself:

- **Visual:** screenshots, layout, colours, typography and positioning.
- **Structural:** the accessibility hierarchy, element roles, labels and relationships.
- **Behavioural:** what happens when you tap things, type sequences, change state.
- **Navigational:** which screens lead where, and which states you can reach.
- **Temporal:** what changes after repeated interactions, a relaunch, or simply time.

Each one reveals something different. None tells the whole story. And you don't need to know in advance which will be useful. You collect it all and find out.

So the accessibility tree is one of our maps, not the edge of the world. We also want the screenshots, the answers to button-mashing nobody sensible would attempt, what survives being closed and reopened, what the app asks of the rest of the phone, and what it fails to tell us at all.

A screenshot tells us what something looks like. The accessibility tree tells us something about what it is. Interaction tests tell us what happens when we press it.

## 1. Survey the terrain

The first job is to find out what is actually there, before deciding what it means.

We use `forge ui tree`, which asks WebDriverAgent (a small test runner that lives on the iPhone) what is on screen, from a Linux laptop over a USB cable. No Mac, no Xcode, no simulator: the real calculator on a real phone. Alongside it, screenshots of the main screen, the history, and the landscape layout.

Instead of simply looking at a button and deciding that it looks like a button, we can ask what the operating system thinks that button is.

Except first we had to get past our own border control. Forge won't read an app it didn't build unless you say `--any-app` out loud and your own policy file lists the app. Around half past midnight, the laptop politely declined to edit that file on my behalf until I typed the words myself. Annoying. Also exactly what it's for.

Then the calculator started talking.

- **Observed:** all twenty keys are there, each with a sensible label and a name underneath: Seven, Divide, Negate, AllClear. Not "Button 14". Somebody at Apple cares.
- **Observed:** the number on the display is in the tree too, but with an invisible left-to-right character glued to the front. And, as we found later, glued in front of *every part* of "12+7". A comparison that doesn't strip these will fail forever and never tell you why.
- **Observed:** Forge can't open Calculator by itself. It only knows apps that were installed, and Calculator just *is*. So the most advanced piece of our tooling was, for this step, me, opening an app with my thumb.

Then I turned the phone sideways, fully expecting the scientific calculator. Everyone knows sideways is the scientific calculator.

- **Observed:** it isn't. Not any more. Sideways is the same basic keypad, stretched into pills and rearranged. My first confident assumption lasted about four minutes.
- **Observed:** and sideways, the answer vanishes. Calculator happily showed 19 on the screen, while the accessibility tree had two neat, empty, invisible boxes where the numbers should be. Turn it upright and the 19 comes back.

The calculator has a portrait mode where it tells you the answer and a landscape mode where it shows you the answer but won't say it. I have no idea why. Into the Unknown column it goes.

- **Observed:** the clear key flips between C and AC, but the tree calls it "All Clear" either way. Only the screenshot knows. And its *position* in the tree moves depending on which one it is.
- **Observed:** close the app from the app switcher, open it again, and 12 + 7 = 19 is still sitting there, with the sum above it. It's in History too.
- **Observed:** History, according to the accessibility tree, is an *alert*. I've been alerted.
- **Unknown:** what the little ">" tab on the left edge of the screen does. It's in every screenshot and never once in the tree.

Seventeen minutes, one phone that fell off the USB cable mid-expedition, twenty-five findings. None of them are hard. All of them would have broken a naive recreation, or a naive test.

I am no longer looking at a calculator. I am interrogating a calculator like it owes me money.

## 2. Collect evidence

A calculator has buttons for numbers and operators. It also has states, input sequences, formatting rules, edge cases, and behaviours that are easy to overlook when you're casually using it.

So we wrote down 39 questions. Not answers: questions. Each is a sequence of keys and the one thing it asks.

Some are obvious:

* Press 7, then +, then 5.
* Enter a decimal.
* Perform a division.
* Enter a long sequence of numbers.

Others are less obvious.

What happens when you press equals repeatedly? What happens when you divide by zero? What happens when you enter an absurdly large number? What happens when you press buttons in an order that a sensible person would never consider?

The original developers have presumably spent a great deal of time deciding what should happen in these situations. We get to discover those decisions the hard way.

This time I wasn't pressing anything. Forge was. A small script turns the 39 questions into one long test, Forge presses every key by its name, and after each sum it reads the answer out of the accessibility tree.

It took four attempts.

The first never pressed a key: Forge refused to believe Calculator existed, because it only knew about apps somebody had installed, and nobody installs Calculator. That one needed a fix in Forge itself, the first change to the platform this expedition caused. The second never pressed a key either: I'd picked up the phone to read the progress messages, the Claude app was in front, and Forge, quite correctly, refused to tap an app it hadn't been asked to touch. The third got through eight sums and then discovered that the "All Clear" key is called "Clear" while there's something to clear. The fourth got through 31 sums before the phone's auto-lock sent it to the home screen.

At which point I asked why we were about to rerun everything. We weren't, in the end. We recorded the last eight and stopped.

Then there was Delete. Forge has a safety rule: it won't press anything labelled "delete" without a person saying yes, because in most apps that's your data. On a calculator, Delete is backspace. To let it run with nobody watching, I had to give Forge full rein over Calculator, and only Calculator, and take it back afterwards. Even the safety rules are evidence: to a policy written for real apps, a calculator's backspace looks exactly like deleting your files.

And the answers? Here is a selection from the notebook, all **Observed**:

- **0.1 + 0.2 = 0.3.** Not 0.30000000000000004, which is what most programming languages will tell you. Whatever Calculator does inside, it isn't naive floating point. (**Inferred**, and the first trap for our rebuild.)
- **2 + 3 × 4 = 14.** It does multiplication first. Plenty of physical calculators would say 20.
- **2 + 3 = = =** gives 11. Equals repeats the last step.
- **50 %** is shown as "50%", not 0.5. And 200 + 10 % is 220: the percent is of the 200.
- Change sign puts the number in brackets: **(-5)**.
- **5 ÷ 0 says "Undefined". 0 ÷ 0 says "Indeterminate".** Somebody at Apple cared enough to tell the two apart.
- Type 18 digits and it keeps all 18. 999,999,999 × 999,999,999 is **9.99999998e17**.
- **7 + ⌫ 5 = gives 75.** Delete didn't just remove a digit. It removed the plus, and the 5 joined the 7.

Thirty-nine sums. Not one of them is hard. At least six would break a recreation written by someone who thought they knew how a calculator works. That someone was me.

- **Unknown:** whether it gives the same answers twice. We only asked once. A calculator surely does, and we decided that proving it was testing for its own sake.

We are effectively writing a field guide to a calculator.

## 3. Build a map

Which screens exist? What controls are available? What happens when we press them? How does navigation work? What changes after an interaction?

For now the map is drawn by hand, from the trees and the recorded table: screens, controls, where each leads, and the rules we think the calculator follows. Every rule gets its label. A rule only becomes Observed when there's a recorded sequence that tests it.

We are effectively writing a field guide to a calculator.

(We do own an automatic map-maker, an explorer that walks an app's screens and draws the graph for us. It's currently parked. Whether it comes back out depends on how far the hand-drawn map gets us.)

## 4. Reconstruct the artefact

Now comes the actual reconstruction.

The intention isn't to generate an app automatically from the accessibility tree. That would be a different project.

Instead, we manually recreate the behaviour, using the evidence as our specification: the arithmetic and its state in Rust, the interface in native SwiftUI. We need to reproduce the available controls, the interaction model, and the important behavioural details. Not the look, though: ours gets its own name and its own buttons. Apple's design is Apple's, and the test doesn't care what colour the equals key is.

The accessibility tree gives us clues. The screenshots give us visual references. The recorded interactions give us tests.

And the rest is engineering.

Or, depending on the afternoon, increasingly elaborate guesswork. Which is the point of this step: finding out which of our assumptions were correct and which were spectacularly wrong.

## 5. Verify the findings

Once the recreation exists, we replay the same interaction sequences against both calculators and compare the results.

Most of this fight doesn't even need the phone. The table we recorded from the original doubles as a test for our calculator's engine, so it runs on Linux in seconds. The phone is for the final round.

We leave the screenshots out of it: the two calculators are meant to look different, so comparing pixels would only tell us what we already know.

The point isn't to produce a perfect clone immediately. It's to establish a repeatable way of finding out where our implementation differs from the original.

If one calculator displays a different result, we investigate.

If one exposes a different set of controls, we investigate.

If one behaves differently after a sequence of inputs that nobody should ever need to perform, unfortunately, we investigate that too.

And whenever something doesn't make sense, we go back to the original and look again. Every difference ends up as one of two things: a bug in our calculator, or evidence that the platform underneath it is missing something.

Two calculators enter. One calculator leaves.

Hopefully.

## Why bother?

A calculator is a deliberately small first specimen.

The more interesting idea is whether an existing application can be treated as a black-box specification, and how much of it can be recovered from the outside.

That could become a useful way to understand unfamiliar applications, explore compatibility, investigate interface behaviour, and build replacements for software where the implementation is unavailable. Eventually the same method could go after applications far more complicated than a calculator. But the important thing is the ability to investigate unfamiliar software systematically, not to rush into automating or replacing it.

There are obvious limitations. An accessibility tree doesn't reveal everything. Screenshots don't explain the underlying logic. And a collection of successful interaction tests doesn't prove that we've reproduced every possible behaviour. That's what the Unknown column is for.

But that's precisely why the experiment is interesting.

And we're starting with a calculator.

A few buttons. A display. Some arithmetic.

Infinite suffering.
