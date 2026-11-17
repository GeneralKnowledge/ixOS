# A Few Buttons, Infinite Suffering

*Or: how rebuilding a calculator became an exercise in digital archaeology, accessibility-tree archaeology, and questioning my life choices.*

I thought rebuilding a calculator would be easy.

There are buttons. There are numbers. There are some operators. You press things, numbers appear, and eventually you get an answer.

How complicated could it possibly be?

As it turns out, a calculator is a deceptively small concentration of suffering.

The idea is simple: take an existing iOS app, inspect it from the outside, understand how it works, and manually recreate it. No source code. No peeking behind the curtain. Just the app itself, its accessibility tree, screenshots, and a collection of scripts to help investigate it.

Essentially, we're treating an ordinary app like a crime scene.

## Step one: interrogate the calculator

The first job is to understand what the original app actually exposes.

Using forge ui tree to inspect its accessibility hierarchy, alongside xtool and a handful of small scripts for the surrounding build and device workflow, we can start building a picture of the application.

The accessibility tree is particularly interesting. It gives us a structured view of the interface: elements, labels, roles, and the controls available to interact with.

Instead of simply looking at a button and deciding that it looks like a button, we can inspect what the operating system thinks that button is.

This matters because an interface is more than pixels.

A screenshot tells us what something looks like. The accessibility tree tells us something about what it is. Interaction tests tell us what happens when we press it.

Together, they give us a surprisingly useful external specification.

Of course, this is where the fun ends.

A calculator has buttons for numbers and operators. It also has states, input sequences, formatting rules, edge cases, and behaviours that are easy to overlook when you're casually using it.

I am no longer looking at a calculator. I am interrogating a calculator like it owes me money.

## Step two: document the crime scene

Before recreating anything, we need to map the application.

Which screens exist? What controls are available? What happens when we press them? How does navigation work? What changes after an interaction?

We'll record the accessibility structure, capture screenshots, and build a collection of interaction sequences.

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

We are effectively writing a field guide to a calculator.

## Step three: build another calculator

Now comes the actual reconstruction.

The intention isn't to generate an app automatically from the accessibility tree. That would be a different project.

Instead, we'll manually recreate the interface and its behaviour, using the information we've gathered as our specification.

This is the interesting part.

We have to make our own implementation behave like the original, despite having no access to its internal code. We need to reproduce the visible interface, the available controls, the interaction model, and the important behavioural details.

The accessibility tree gives us clues. The screenshots give us visual references. The recorded interactions give us tests.

And the rest is engineering.

Or, depending on the afternoon, increasingly elaborate guesswork.

## Step four: make the two calculators fight

Once the recreation exists, we can replay the same interaction sequences against both applications.

We can compare the resulting values, inspect their accessibility structures, and examine screenshots to identify differences.

The point isn't necessarily to produce a perfect clone immediately. It's to establish a repeatable way of finding out where our implementation differs from the original.

If one calculator displays a different result, we investigate.

If one exposes a different set of controls, we investigate.

If one behaves differently after a sequence of inputs that nobody should ever need to perform, unfortunately, we investigate that too.

Two calculators enter. One calculator leaves.

Hopefully.

## Why bother?

A calculator is a deliberately small starting point.

The more interesting idea is whether an existing application can be treated as a black-box specification.

We can inspect its accessible interface, map its navigation, record interactions, and manually recreate its behaviour without needing the original source code.

That could become a useful way to understand unfamiliar applications, explore compatibility, investigate interface behaviour, and build replacements for software where the implementation is unavailable.

It also gives us a practical way to test how much information we can recover from the outside of an application.

There are obvious limitations. An accessibility tree doesn't reveal everything. Screenshots don't explain the underlying logic. And a collection of successful interaction tests doesn't prove that we've reproduced every possible behaviour.

But that's precisely why the experiment is interesting.

We're trying to discover how much of an application can be understood from what it reveals to the outside world.

And we're starting with a calculator.

A few buttons. A display. Some arithmetic.

Infinite suffering.
