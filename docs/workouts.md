# Workouts

The **Workouts** tab on the start page sets up a workout (R56): instead of following the road,
the trainer holds a power (ERG mode).

- **Constant power** — the trainer holds the power you set, e.g. 200 W.
- **Heart-rate zone** — Torqa holds your heart rate in the middle of a zone (zone 3 of a 185 bpm
  maximum: 139 bpm) by setting the power for you.
- **Heart rate** — the same for a heart rate you choose, in bpm.

![The Workouts tab](images/workouts/tab.png)

The zones are your rider's (from their maximum heart rate, [riders.md](riders.md)); new values
start from your FTP. A heart-rate workout needs a heart rate: a strap, or the fake trainer's
simulated one ([riding.md](riding.md)).

## Holding a heart rate

Heart rate follows power only after 30–60 s, so Torqa changes the power gently:

- It starts at the **lowest power** you set and rises by at most 30 W a minute.
- It never goes below the lowest or above the **highest power**. If your target needs more than
  the highest power, the trainer stays there.
- Without a heart rate (strap off, or dropped out) the power stays where it is until the heart
  rate is back.
- As your heart rate drifts up over a long ride, it eases off.

Expect to reach the target after about 5–10 minutes. The lowest power is also your warm-up:
set it to something you can ride easily.

## Where

- **On its own** — no route: the screen shows your HUD, the workout's targets and a live chart
  of power and heart rate. Speed and distance are those of a flat road at your power.
- **On a course** — any course of your library with a place (not Tacx RLV videos), ridden in
  its 3D world. The trainer holds the workout's power while the course's gradient sets your
  speed; the world looks as last set on the course page. The ride ends at the finish.

**Start** connects the trainer chosen under Devices & Settings, builds the course's world if
needed, and starts.

## During the workout

![Holding zone 3 on its own: the heart rate settles on the dashed target line](images/workouts/heart-rate-hold.png)

The panel at the top right shows what the workout asks for: the power the trainer holds and, for
a heart-rate workout, the heart rate it aims at and your heart rate now. The settings dialog
(**S**) has a **Workout** tab to change it as you ride, e.g. another zone or more power; the
change reaches the trainer at once. **Finish & save** puts the workout into the history: on its
own under its name (e.g. "Heart-rate zone 3"), on a course under the course's name. Workouts on
their own are saved as indoor cycling FIT files without GPS positions.

The command line rides the same workouts: see [cli.md](cli.md#ride-a-workout).
