# Ride HUD

The figures on the left of the ride screen are yours to choose, per rider. Edit them in
**Rider settings → HUD** (the **Profile** tab → the pencil), or with
**Settings → HUD** while riding (button or key S) — the ride keeps going, and changes are
saved to the rider.

The editor shows the **HUD** itself with example values, and the figures still **available**.

- **Drag a figure straight into the HUD** to where it should appear: a blue line shows where
  it will land — before or after the figure under the pointer (left/right in the grid,
  above/below the large figure). Dropping on free space adds it at the end; + does the same
  with a click.
- **Move** figures by dragging them within the HUD, and **drag them back** onto the available
  list to remove them.
- The **first** figure is shown large at the top; with power figures, your W/kg, power in % of
  your FTP and power zone appear below it. Up to 13 figures in total.

![The HUD editor in the ride settings, a figure being dragged into the HUD](images/app/hud-editor.jpg)

| Figure | Meaning |
|---|---|
| Power, Power 3 s, Power 10 s | Instant power and its average over the last 3 or 10 seconds |
| Avg power, Normalized power | Over the ride so far ([history.md](history.md) explains NP) |
| W/kg, Power % FTP, Power zone | Power per kilogram body weight, in percent of your FTP, and its zone |
| Heart rate, Heart rate % max, Heart-rate zone | From a heart-rate strap, in percent of your maximum heart rate, and its zone |
| Cadence | Pedal revolutions per minute |
| Gear | The virtual gear (1–24) on a single cog ([riding.md](riding.md#virtual-gears)) |
| Speed, Avg speed | Virtual speed |
| Distance, To go | Ridden so far, left to the finish |
| Time, Time to go | Since the start (pauses do not count), and estimated to the finish (below) |
| Elevation, Climbed, Ascent to go | Current altitude, climbing so far, climbing still ahead on the route |
| Grade, Next 500 m | Gradient here, and on average over the next 500 m |
| Intensity, TSS, Work | Intensity factor, training stress score and kilojoules so far |

**Time to go** rides the rest of the route through the same physics as you, climbs and
descents included, at the power you are expected to hold: 75 % of your FTP at the start (as
the course page estimates), moving over to your average power of the ride during its first
ten minutes. It is renewed every ten seconds and counts down in between. Without a route, or
while that power would not get you to the finish, it shows `--`.

The percentages are on the scale your zones are set on (**Rider settings → Zones**, see
[riders.md](riders.md)): at 92 % FTP you are just past the top of the standard zone 3, 90 %.

Each rider has their own layout, stored in `profiles/<rider>/hud.toml`. Units follow the rider's
profile (metric or imperial).
