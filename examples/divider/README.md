# divider

The walking-skeleton example: the smallest end-to-end kitest run, proving the
KiCad export seam. It doubles as a regression fixture.

## The design

A resistor divider, drawn with **no SPICE parts and no analysis directive** in the
schematic (nothing test-specific lives in the design):

- `R1`, `R2` in series.
- A labeled input net `VIN` (the driven port).
- The midpoint labeled `out` (the read port).
- A GND power symbol, so the ground net exports as SPICE node `0`.

The nets to drive and read are **labeled** so they export as stable node names;
unlabeled nets get fragile auto-generated names.

## The test

kitest drives the design through its ports, test-side:

1. Export the schematic with `kicad-cli sch export netlist --format spice`.
2. Append the stimulus and analysis kitest owns: `V1 VIN 0 5` and `.op`.
3. Run through the ngspice backend, read node `out`.
4. Assert `out ~= 5 * R2 / (R1 + R2)` within a tolerance (no exact-float compare).

## Files

- `divider.kicad_pro`, `divider.kicad_sch` (committed source)
- `divider.spice` (committed golden netlist; detects kicad-cli export drift, and
  lets tests run without kicad-cli)
- `test_divider.py` (the Python testbench)
