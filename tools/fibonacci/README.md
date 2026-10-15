# Fibonacci

The generator for the built-in Fibonacci pack.

```sh
python3 tools/fibonacci/build.py packs/fibonacci
```

Six views of the Fibonacci numbers, 30 seconds each, in the theme's colors, told in pictures rather than words:

1. **Squares and the spiral:** squares of Fibonacci sides tiling a golden rectangle, a quarter circle through each,
   seen from the first tiny square as the view draws back.
2. **The sunflower:** petals opening around a thousand seeds, each turned by the golden angle; then its 21 spirals one
   way light up, and its 34 the other.
3. **The rabbits:** Fibonacci's own puzzle, month by month: each grown pair has a young pair, and the young grow up.
   Each pair is two rabbits, hopping in as they're born.
4. **Pascal's triangle:** each number a disc as large as it is; its shallow diagonals light up in turn, and each one's
   sum grows as a bar: the Fibonacci numbers.
5. **Toward the golden ratio:** each number over the one before, closing in on φ, beside a rectangle of each ratio's
   shape, settling into the golden rectangle.
6. **The Fibonacci word:** a walk along 0100101001001…, turning at each 0.

Everything is worked out here, exactly, and written as data and simple expressions. Needs only Python 3.
