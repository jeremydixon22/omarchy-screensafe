# Flash temporal reference fixtures

`flash-temporal.txt` contains unchanged 8-bit RGB sequences and pass/fail labels
from Trace RERC's [pse-test-media](https://github.com/traceRERC/pse-test-media),
commit ef78542aad496827f25064f57aaf13d6a741ba75.
The BSD-3-Clause license is reproduced in `LICENSE-pse-test-media`.

The 98 cases come from `trace24_30fps_01`, `trace24_30fps_red01`,
`trace24_30fps_red02` and `30fps_alternating_01`. Area-pass cases are excluded.
Each remaining single-area temporal pattern is applied uniformly to our grid,
at the original 30 FPS. This preserves the expected count/intensity outcome
while removing display-geometry differences. This is a temporal benchmark,
not a claim to pass the complete spatial suite or certify medical safety.

Cases include irregular counts, multistep/reversed transitions, one subthreshold
flash within a sequence, chromaticity and saturation boundaries. The fixture
format starts each case with its original name and expected failure (0 or 1),
followed by one RGB triple per source frame.

The source suite accompanies Jordan, J. B. (2025), *Evaluating Conformance of
Video Safety Tools for Photosensitive Epilepsy*,
https://doi.org/10.1007/978-3-031-93848-1_7.
