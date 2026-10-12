# Analysis

The **Analysis** tab on the start page looks at your power across all your rides, where the
[history](history.md) looks at one ride at a time. It shows the active rider's rides; switch
riders on the Profile tab.

![The Analysis tab: the power curve of the last six weeks over the six weeks before, read off at 5 minutes](images/app/analysis.png)

## Power curve

The power curve shows the best average power you held for every length of effort: your best
5 seconds on the left, your best 20 minutes in the middle, your best hours on the right. The time
axis is logarithmic, so short efforts get as much room as long ones. Hover over the chart to
read off each curve at that duration, with the date of the ride it comes from.

- **Period**: the last 6 weeks, 90 days or 12 months, or all time. The curve in blue is the
  best of the rides in that period.
- **Compared with**: in grey, the best of all your rides, or of the period just before (the
  6 weeks before the last 6). Where the blue curve reaches the grey one, you are as strong as
  you have ever been; where it stays under it, there is form to win back.
- **Per kilogram**: the curve in W/kg instead of watts, each ride with the weight you had then.
  Rides saved before Torqa kept the weight use your weight today.
- Under the chart: your best 5 s, 1 min, 5 min, 20 min and 60 min of the period, and of the
  rides compared with in grey.

Simulated rides (sped up or moved along with the fake trainer) count for nothing here: the
power was not yours.

## Records

A ride holding your best power over 5 s, 1, 5, 20 or 60 minutes gets a ★ for it on its summary
and in the history, e.g. *★ Best 5 min: 312 W*. Another ride beating it later takes the ★ over.

## How it is computed

Each ride keeps its best power over 36 durations, from 1 s to 8 h, in its summary file beside
the FIT file, so the tab reads no FIT files and the data syncs with the rides
([ADR 0014](adr/0014-ride-analysis-data.md)). Rides saved before this existed, and FIT files
copied in by hand, are read once the next time the history or this tab opens.

Seconds without power count as 0 W. In FIT files from other apps or devices, power is taken
once per second; a gap of more than 10 s between records counts as a pause, and no effort spans
it.
