# Expressions

Wherever a pack setting moves or changes, it's an expression: a small formula such as `0.4 * sin(t * 2)` or `t > 3 ? accent : muted`. Expressions read the [names a pack can use](PACKS.md#names-you-can-use) and give back one value: a number, a color, or true/false.

They can only compute: an expression can't loop, call itself, define functions, keep anything from one frame to the next, or read or write anything at all. Every expression is checked and compiled when the pack loads. Whatever values it meets, it finishes quickly with a value: nothing it computes is ever infinite or "not a number".

## Values

- **Numbers:** `3`, `0.25`, `.5`, `1e-3`. Expressions work in double precision.
- **Colors:** `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`, a palette name such as `accent`, or a color function. Channels run from 0 to 1.
- **True and false:** `true` and `false`, or a comparison.

Each setting wants one kind of value: a position wants a number, `color` a color, `visible` a true/false. A pack that gives the wrong kind doesn't load. A `let` name takes the kind of its expression.

## Operators

From loosest to tightest:

| Operator | Meaning |
|---|---|
| `c ? a : b` | `a` when `c` is true, otherwise `b`; `a` and `b` must be the same kind. |
| `a \|\| b` | Either is true. |
| `a && b` | Both are true. |
| `<` `<=` `>` `>=` `==` `!=` | Compare two numbers (`==` and `!=` also compare two true/false values). Exact, and can't be chained: write `0 < x && x < 1`. |
| `+` `-` | Add, subtract. |
| `*` `/` `%` | Multiply, divide, remainder. |
| `-a` `!a` | Negate a number; negate a true/false. |
| `a ^ b` | Power, grouping to the right: `2 ^ 3 ^ 2` is `2 ^ 9`, and `-2 ^ 2` is `-(2 ^ 2)`. |

## Arithmetic never fails

- **Division by zero:** dividing by zero gives 0, and so does a remainder by zero.
- **Remainder:** `%` and `mod()` take the sign of the divisor, so `-7 % 3` is 2.
- **Results outside the numbers:** any result that isn't a finite number becomes 0. So `sqrt(-1)`, `log(0)`, `(-8) ^ 0.5` and `exp(1000)` all give 0.
- **Out-of-range inputs:** `asin` and `acos` clamp their input to −1…1.
- **Colors:** color channels are clamped to 0…1.

## Functions

| Function | Gives |
|---|---|
| `sin(x)` `cos(x)` `tan(x)` | Trigonometry, in radians. |
| `asin(x)` `acos(x)` `atan(x)` `atan2(y, x)` | Inverse trigonometry. |
| `sqrt(x)` `exp(x)` `log(x)` `pow(x, y)` | Square root, eˣ, natural logarithm, power. |
| `abs(x)` `sign(x)` `floor(x)` `ceil(x)` `round(x)` `trunc(x)` `fract(x)` | Rounding and parts of a number; `fract(x)` is `x - floor(x)`. `int(x)` is `trunc(x)`. |
| `sqr(x)` | x². |
| `mod(a, b)` | `a % b`. |
| `min(a, b, …)` `max(a, b, …)` | Smallest or largest of 2 to 16 numbers. |
| `clamp(x, lo, hi)` | x kept between lo and hi (either order). |
| `mix(a, b, t)` | From a to b as t goes from 0 to 1: numbers (t may go beyond, to extrapolate) or colors (t is kept to 0…1). |
| `step(edge, x)` | 1 when x ≥ edge, otherwise 0. |
| `smoothstep(e0, e1, x)` | A smooth step from 0 to 1 as x goes from e0 to e1. |
| `remap(x, a, b, c, d)` | x moved from the range a…b to c…d, not clamped. |
| `interp(x, x0, y0, x1, y1, …)` | Straight lines through 2 to 16 points (x0 < x1 < …), held level beyond the first and last: keyframes. |
| `ease_in(u)` `ease_out(u)` `ease_in_out(u)` | Cubic easing of u, kept to 0…1. |
| `sigmoid(x, k)` | `1 / (1 + e^(−x k))`. |
| `tri(x)` `saw(x)` | Triangle and sawtooth waves, 0 to 1, repeating every 1. |
| `noise(x)` `noise2(x, y)` | Smooth noise, 0 to 1, the same every time. |
| `hash(x)` | A value from 0 to 1 that depends only on x: the same every time. |
| `rand(x)` | Like `hash`, but different each time the screensaver opens. |
| `pick(i, a, b, …)` | The choice numbered `floor(i)` among 2 to 16 numbers or colors, counting from 0 and going round, so `pick(i, red, green)` alternates. |
| `rgb(r, g, b)` `rgba(r, g, b, a)` `gray(v)` | Colors from channels, 0 to 1. |
| `hsv(h, s, v)` | A color from hue (in turns: 0, 1/3 and 2/3 are red, green and blue), saturation and value. |
| `alpha(c, a)` | Color c with its opacity multiplied by a. |

## Naming values

A scene or element `let` list names values in order, each usable by the ones after it:

```toml
let = ["phase = t * 0.18", "r = 0.3 + 0.05 * sin(phase)", "tint = r > 0.32 ? accent : muted"]
```

A name can't be redefined or reuse a built-in name, so a mistyped name is always an error. ScreenSafe suggests the closest name it knows:

```text
scene "Orbits" › draw 2 › color: unknown name 'acent' (did you mean 'accent'?) (at character 1)
    acent
    ^
```

## Repeating without loops

There are no loops. Things repeat through settings fixed when the pack loads:

- `count` draws copies of an element, numbered by `i`.
- `samples` evaluates `px` and `py` along a line, at `s` from 0 to 1.
- A mesh's `edge_color` is worked out once per edge.

Their cost is multiplied out when the pack loads, and a scene that would do too much a frame is refused. The cost of an expression is the number of operations it compiles to, and `check` reports each scene's total.

## Compared with MilkDrop

The language borrows ideas from MilkDrop's preset equations, which are written in EEL2 from Cockos's WDL library:

- equations as the way to animate;
- names such as `sqr`, `sigmoid` and `int`.

It shares no code with MilkDrop or EEL2. Some of EEL2's choices suit a music visualizer running presets its user chose, but not screensaver packs from anyone, so ScreenSafe changes them:

| EEL2 | ScreenSafe |
|---|---|
| Compiles equations into native machine code. | Compiles to a list of operations, read by an interpreter written in memory-safe Rust. Nothing becomes machine code. |
| A variable exists as soon as it's used, starting at 0, so a misspelt name silently reads 0. | Every name must already exist; a misspelling is an error that suggests the right name. |
| `==` treats numbers within 0.00001 of each other as equal. | Comparisons are exact. |
| `loop()` and `while()` run code repeatedly, stopping after 1,048,576 rounds. | No loops: `count` and `samples` repeat, fixed and costed when the pack loads. |
| Values can carry over from frame to frame, so motion written per frame can depend on the frame rate. | Every value is worked out from the time alone: the same picture at any frame rate, and any moment can be drawn directly. |
| Blocks of memory (`megabuf`, `gmegabuf`) shared by code. | No memory beyond named values. |
