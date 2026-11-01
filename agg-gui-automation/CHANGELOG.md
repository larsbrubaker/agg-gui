# Changelog

All notable changes to `agg-gui-automation` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

## [0.1.1] - 2026-11-01

### Changed

- On the virtual clock, a condition wait (`wait_until`, `wait_for`, `assert`, `wait_for_name`,
  `wait_for_name_with`, `wait_for_widget_disappear`, `wait_for_widget_enabled`, `wait_for_image`,
  the image gestures' search, and `wait_for_idle`, whose condition is idle) sleeps one frame
  interval (10 ms) of real time before each frame it pumps while its condition is false and the
  background-busy hook reports busy, so it can no longer give up before the worker thread it waits
  on delivers, and a long `wait_for_idle` no longer races its virtual clock through its whole
  budget (firing hover, tooltip and double-click timers) while a worker runs. A wait whose
  condition already holds, or that runs with no background work, sleeps nothing; one whose
  condition comes true between two looks has paced the frames up to that look (about 50 ms for the
  name waits). `delay`, `wait_for_pending_ui_work`, the gestures' pauses and
  `HeadlessDriver::pump` keep taking no real time: they measure UI time, not the arrival of a
  result.
