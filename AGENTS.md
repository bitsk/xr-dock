# XR Dock

Maintain the independent scene client, app catalog and task intent model here.
Use xr-shell-protocols from its owning repository and xr-spatial-kit widgets;
do not copy the protocol or renderer. Shell snapshots are the task/workspace
authority. Dock never starts an OpenXR session or directly controls Wayland
objects, hardware or a second OS lifecycle service. Installing the package
must not launch XR. Preserve real identities/dates and independent packaging;
keep private paths, addresses and credentials out of public files.

## XR Spatial Engineering Constraints

Follow `openkylin-xr-docs/wiki/xr-spatial-guidelines-and-issues.md`:
- **Anchor-Relative Popups & Ergonomic Angles**: Never hardcode the app launcher or window picker at fixed world origin coordinates with zero tilt. Spawn and orient popups relative to the active Dock anchor or current user horizon yaw with comfortable pitch compensation.
- **Ray Hit-Target Minimums**: Every interactive Dock button, pagination control, and window picker item must meet the `>= 2.2°` (`>= 44 dmm`) minimum angular hit target size.
