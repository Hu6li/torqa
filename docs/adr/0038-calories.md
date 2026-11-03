# ADR 0038 — Calories in the ride HUD

- Status: accepted
- Date: 2026-11-03
- Requirement: R193; issue #475

## Context

The Work figure integrates measured power and shows mechanical work in kJ. Riders also want
the familiar kcal figure for estimated metabolic energy. Converting mechanical work directly
to kcal would answer a different question from calories burned.

## Decision

- Add a separate **Calories** metric in **kcal**, selected through the existing per-rider HUD
  layout. Keep Work and the default layout. Both can be displayed, also in the overlay.
- Keep energy in joules in Rust. Convert to kcal at the HUD presentation boundary using
  1 kcal = 4,184 J. Distance unit settings do not affect calories.
- Choose one source for each recorded one-second sample, in this order:
  1. Finite nonnegative measured power, divided by a fixed gross efficiency of 24%. Zero
     watts is a valid measurement and does not fall back to heart rate.
  2. Heart rate with explicitly supplied age, sex and body weight: Keytel's sex-specific
     regression without VO₂max. The regression returns kJ/min; convert to J/s and clamp
     negative predictions to zero. Require an adult age of 18–100, body weight 30–200 kg
     and a finite heart rate of 30–250 bpm. These bounds reject unusable inputs; they are
     not a claim that the model has been individually validated throughout those ranges.
  3. An explicitly selected assumed effort, using stationary-cycling values from the 2024
     Compendium: Easy 3.5 MET (01210), Moderate 6.8 MET (01200), Hard 10.8 MET (01236).
     Estimate kcal as MET × body weight in kg × recorded hours.
- Age and sex start unset; an assumed effort starts off. The avatar, FTP, virtual speed,
  course grade and workout target never supply missing physiological measurements.
- With no estimable second, show `--`. If only some seconds can be estimated, show their
  total and disclose the missing intervals in the tooltip. The tooltip also identifies
  power, heart rate, assumed effort or mixed sources; the caption stays **Calories**.
- Accumulate only recorded samples. Pauses and gaps in timestamps add no extra intervals.
  Cache the result incrementally, so reading the HUD does not scan an entire ride each frame.
  Editing calorie inputs or body weight recomputes the ride so far, including while paused.
- Save optional inputs under `[calories]` in the rider's `profile.toml`. Older profiles have
  no fallback until the rider opts in. No new dependency, network request, ride-file field
  or FIT-export change is needed for this HUD feature.

## Meaning and limits

This is an estimate of energy used while riding, not a measurement of metabolism or a daily
calorie total. The HR and MET models include the metabolic cost represented by their source
models; no separate resting-calorie allowance is added. Do not label this **Active calories**
or claim equivalence to Garmin's activity/active calorie totals. A power reading of zero
contributes zero in the power-based approximation.

Heart rate depends on more than effort and the regression is a population model, not Garmin's
proprietary Firstbeat algorithm. The assumed-effort option is deliberately rough and counts
recorded time even without pedalling; riders must pause when they stop. No source promises
agreement with another platform or a measured individual energy expenditure.

## Sources

- [NIST's calorie definitions](https://www.nist.gov/pml/special-publication-811/nist-guide-si-footnotes).
- [TrainingPeaks on work, efficiency and calories](https://www.trainingpeaks.com/coach-blog/predicting-measuring-kilojoules-trainingpeaks/).
- Keytel et al., [*Prediction of energy expenditure from heart rate monitoring during
  submaximal exercise*](https://pubmed.ncbi.nlm.nih.gov/15966347/), Journal of Sports Sciences
  (2005), DOI 10.1080/02640410470001730089. The coefficients without VO₂max
  are also given in [Keytel's thesis, chapter 4](https://open.uct.ac.za/bitstream/handle/11427/19142/thesis_hsf_2004_keytel_lara.pdf?sequence=1).
- [2024 Compendium, bicycling](https://pacompendium.com/bicycling/) and its
  [MET definition](https://pacompendium.com/).

## Validation

Core tests check worked power/HR/MET examples, zero watts, required inputs, invalid values,
source changes, missing intervals, cached reads, timestamp gaps and profile edits. Storage
tests cover old files, unknown optional choices, independent avatars and round trips. Godot
checks cover the short caption, kcal in either distance unit system, unavailable values,
tooltips, HUD selection and profile inputs across the Rust boundary. Screenshot fixtures
show the tile and its setup in English and German without generating scenery.
