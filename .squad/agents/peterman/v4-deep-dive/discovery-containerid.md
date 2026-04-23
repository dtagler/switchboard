# Why We Don't Trust ContainerId on Surface Internal Hardware

We believed ContainerId would be our composite-device safety primitive. It isn't. Here's what Spike 2 found—and why the v4 Layer 3 architecture had to abandon it.

## The Setup: What ContainerId Was Supposed to Mean

In Windows device management, a **ContainerId** is a GUID that groups physically-related devices into a single logical unit. Microsoft's design intent: when you have a multi-function peripheral (say, an all-in-one printer with scanner, fax, and copier), all three devices share a ContainerId so the OS and applications know they're one physical thing. You query one device's ContainerId, you can identify all its peers.

For our safety model, this was perfect. The v4 Layer 3 architecture wanted to refuse any disable operation on a device whose ContainerId matched the touchpad's ContainerId. The logic: if the internal keyboard and touchpad shared a ContainerId (indicating a composite device), disabling one child without understanding the tree structure could accidentally cripple the other. ContainerId became our guard rail.

Microsoft documents this behavior in the [Device Container Identification](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/device-container-id) guidance for hardware manufacturers—the expectation being that composite devices will populate ContainerId consistently across all child nodes.

## The Discovery: The Sentinel Everywhere

Spike 2 ran read-only PowerShell enumeration on the owner's Surface Laptop 7 (Snapdragon X Elite). The result was stark:

- **Surface HID Keyboard:** ContainerId = `{00000000-0000-0000-FFFF-FFFFFFFFFFFF}`
- **Surface HID Mouse (touchpad):** ContainerId = `{00000000-0000-0000-FFFF-FFFFFFFFFFFF}`
- **All four button-collection HID devices:** ContainerId = `{00000000-0000-0000-FFFF-FFFFFFFFFFFF}`
- **VHF power-button injection device:** ContainerId = `{00000000-0000-0000-FFFF-FFFFFFFFFFFF}`

That value—`{00000000-0000-0000-FFFF-FFFFFFFFFFFF}`—is Windows's sentinel for "device does not belong to a container." It's the device-management equivalent of `null`. On this hardware, *every* internal device reports the sentinel, defeating the safety primitive entirely.

A ContainerId-based safety check becomes useless when the check is: "refuse if ContainerId matches the touchpad's sentinel value." Every device on the Surface matches. No information. No protection.

## Why This Happens: The Surface Aggregator Module (SAM)

Surface Laptop 7 internal hardware—keyboard, touchpad, power button, fingerprint reader, sensors, lights—doesn't connect via traditional USB or PCI buses. Instead, Microsoft's **Surface Aggregator Module (SAM)**, a low-power embedded controller, exposes these devices through ACPI (Advanced Configuration and Power Interface) under a single ACPI parent.

SAM child devices on Surface are ACPI-enumerated but don't inherit the composite-device metadata that would populate ContainerId. The standard Windows ContainerId mechanism is keyed off Plug-and-Play container descriptors in device tree—metadata that SAM-bus child devices apparently don't populate. Result: every SAM child gets the sentinel value by default, as if each were an orphan root device.

This is likely-by-design: SAM is a custom embedded controller architecture that predates (or sidesteps) the generic composite-device ContainerId scheme. Microsoft's HAL abstracts the differences away for most consumers, but from a low-level device-enumeration view, Surface's architecture is hardware-specific enough that generic Windows abstractions don't map cleanly.

## The Replacement: Positive HardwareId Allowlist + Parent-Prefix Matching

v4 Layer 3 is now a **positive HardwareId-substring match** combined with **Parent-prefix allowlist**. Specifically:

- **Target device HardwareId must contain:** `Target_SAM&Category_HID&Col01` (the internal keyboard's actual hardware ID substring on Snapdragon X)
- **Parent device must have prefix:** `{2DEDC554-...}\Target_SAM` (the SAM controller's device instance prefix)
- **Cross-check at runtime:** Enumerate the target's parent; confirm it's the SAM parent, not the touchpad's parent (which is ACPI, distinct tree).

On Surface Laptop 7, the keyboard parent and touchpad parent are structurally separate. Keyboard lives under the SAM controller; touchpad lives under its own ACPI device. The cascade risk Layer 3 was protecting against—accidentally disabling the touchpad via a shared composite container—does not exist via this inheritance path. HardwareId pattern + parent structure are what actually identify a device on this box.

## The Lesson: Hardware Specificity Over Generic Windows Abstractions

Don't assume generic Windows abstractions work reliably on Surface hardware without testing on the actual device. ContainerId, `Service` name (e.g., "HidUSB"), and HID class membership are too broad and assume standard USB/PCI device tree semantics. Surface's SAM architecture is idiosyncratic enough that you end up fishing in a sea of sentinel values.

The hardware-specific HardwareId pattern—the actual string Windows uses to identify a device during PnP enumeration—is what you can trust. If you ever feel tempted to "simplify" by replacing the HardwareId allowlist with a ContainerId check, **re-read this document first.** You'll be reverting to a safety net that has holes in it.

## For Future Agents & Maintainers

If you encounter SAM bus, ContainerId, or Layer 3 logic again:

- **The sentinel `{00000000-0000-0000-FFFF-FFFFFFFFFFFF}` means "no container info"**—not "no container." It's a flag value, not a data absence.
- **Every Surface SAM-bus child reports the sentinel** (as of Surface Laptop 7 / Snapdragon X Elite, April 2026 firmware). This is current empirical fact.
- **Trust HardwareId substring matching + Parent-prefix matching** as primary identification logic. This is what actually discriminates devices on this hardware.
- **Use ContainerId only when both endpoints have NON-sentinel values.** For example, a Bluetooth Nuphy Air75 reports ContainerId `{DFCA93C1-...}`—a real GUID, not the sentinel. In that case, ContainerId matching between two non-sentinel devices is meaningful. But on Surface internal hardware, forget it.
- If you're tempted to add "or use ContainerId as a fallback," check the actual Spike 2 output first. You may find yourself leaning on a primitive that provides zero information.

---

**Document status:** Permanent discovery record. If you find contradictory data on newer Surface hardware, create a new discovery entry with date and hardware model. Don't silently revert the HardwareId logic without documenting why.
