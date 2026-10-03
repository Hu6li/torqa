# Ride history

Every finished ride is saved as a FIT file (for Strava, intervals.icu, Garmin Connect, …) in
the rider's folder, `profiles/<rider>/rides/` in the Torqa data directory, together with a small
JSON file holding its summary.

Open **History** on the setup screen, or press **View summary** after a ride:

- **Figures**: time, distance, climbing, average speed, average / normalized / maximum power,
  intensity factor, training stress score (TSS), work in kJ, average heart rate and cadence.
- **Chart**: power and heart rate over the ride, on top of the elevation.
- **Time in zones**: power zones from your FTP, heart-rate zones from your maximum heart rate
  (see [riders.md](riders.md)). Sections without data, e.g. no heart-rate strap, are hidden.
- **Delete ride** removes the FIT file and its summary.

Normalized power needs at least 30 s of power data. Intensity and TSS use the FTP the rider had
when the ride was saved, so they do not change when you update your FTP later; time in zones
uses the current profile.

FIT files copied into a rider's `rides` folder by hand appear in the history too; their summary
is computed and saved the first time the history is opened.
