# Ride HUD

The figures on the left of the ride screen are yours to choose. Press **Customize HUD** while
riding (the ride keeps going) and pick:

- a **large figure** at the top — with power figures, your W/kg and power zone appear below it;
- up to 12 more figures in the grid, shown in the order of the list.

| Figure | Meaning |
|---|---|
| Power, Power 3 s, Power 10 s | Instant power and its average over the last 3 or 10 seconds |
| Avg power, Normalized power | Over the ride so far ([history.md](history.md) explains NP) |
| W/kg, Power zone | Power per kilogram body weight, zone from your FTP |
| Heart rate, Heart-rate zone | From a heart-rate strap, zone from your maximum heart rate |
| Cadence | Pedal revolutions per minute |
| Speed, Avg speed | Virtual speed |
| Distance, To go | Ridden so far, left to the finish |
| Time | Since the start |
| Elevation, Climbed | Current altitude, climbing so far |
| Grade, Next 500 m | Gradient here, and on average over the next 500 m |
| Intensity, TSS, Work | Intensity factor, training stress score and kilojoules so far |

Each rider has their own layout, stored in `profiles/<rider>/hud.toml`. Units follow the rider's
profile (metric or imperial).
