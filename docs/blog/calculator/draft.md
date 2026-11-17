# A Few Buttons, Infinite Suffering

*Or: how rebuilding a calculator became an exercise in digital archaeology, accessibility-tree archaeology, and questioning my life choices.*

<!-- Kept in step with what actually happened: see log.md next to this file. As of 2026-11-17 nothing has run yet, so everything from "1. Survey the terrain" on is still the plan. -->

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

Our notebook before setting off:

- **Unknown:** whether we can read Apple's own app this way at all. Nobody on the project has tried an Apple app yet.
- **Unknown:** whether every key has a label, and whether the display's number can be read from the tree rather than squinted at in a screenshot.

I am no longer looking at a calculator. I am interrogating a calculator like it owes me money.

## 2. Collect evidence

A calculator has buttons for numbers and operators. It also has states, input sequences, formatting rules, edge cases, and behaviours that are easy to overlook when you're casually using it.

So we collect: labels, roles, values, how elements relate, and above all what each interaction does. That last part becomes a table of key sequences, each with the number it leaves on the display.

Some are obvious:

* Press 7, then +, then 5.
* Clear the display.
* Enter a decimal.
* Perform a division.
* Enter a long sequence of numbers.

Others are less obvious.

What happens when you press equals repeatedly? What happens when you divide by zero? What happens when you enter an absurdly large number? What happens when you press buttons in an order that a sensible person would never consider?

These are important questions.

The original developers have presumably spent a great deal of time deciding what should happen in these situations. We get to discover those decisions the hard way.

Then we record the whole table twice. If the calculator gives different answers to the same questions, that's a problem for us and an interesting evening for somebody at Apple.

- **Inferred:** a calculator is deterministic. Same keys, same display. The double recording is how this gets promoted to Observed, or embarrassingly demoted.

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
