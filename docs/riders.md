# Riders

Each rider has a profile: name, weight, bike weight, FTP, maximum heart rate and units
(metric or imperial). Pick the rider in the **Profile** tab; *Edit…* changes the
profile, *New rider…* in the list adds one.

- **Weight + bike weight** set how hard climbs are and how fast you roll.
- **FTP** sets the power zones shown under the power figure (Coggan's seven zones:
  Recovery < 55 %, Endurance ≤ 75 %, Tempo ≤ 90 %, Threshold ≤ 105 %, VO2max ≤ 120 %,
  Anaerobic ≤ 150 %, Neuromuscular above), together with watts per kilogram.
- **Maximum heart rate** sets five heart-rate zones (≤ 60, 70, 80, 90 % and above).
- **Units** switch speed, distance and elevation between km/h, km, m and mph, mi, ft.

Profiles are stored in the data directory as `profiles/<rider>/profile.toml` and can be edited by
hand; each rider's activities are saved in `profiles/<rider>/rides/`. Courses are shared by all
riders.
