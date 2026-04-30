# Screen Light
Portable screen dimmer for multi-monitor setups.

Uses DDC/CI to dynamically dim inactive monitors based on cursor position.

## Usage
Download *screen-light.exe* from the [releases](https://github.com/JeremyJenki/Screen-Light/releases) page. Screen Light automatically generates an adjacent `config.yaml` on first run.

Brightness and delay are both easily changed in the config. The tray menu also includes an option to auto-start with Windows.

For a clean uninstall, disable `Auto-start` and delete *screen-light.exe* and its config file.

> [!NOTE]
> Intended for monitors with DDC/CI support -- it won't have any effect otherwise.

### Supported CLI flags
`--toggle` `--enable` `--disable` `--reload` `--exit`

## Configuration
```yaml
idle_delay_seconds: 1
active_brightness: 75
inactive_brightness: 0

# Win+Shift+B to toggle Screen Light on/off.
hotkey_enabled: true
```
