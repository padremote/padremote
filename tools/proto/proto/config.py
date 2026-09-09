"""Config loading, shared with the shipped desktop app (plan section 9.9).

The prototype deliberately reads the SAME file the Rust app will ship
(desktop/config.default.json), seeded into the user config location, so every
number tuned here is the number that ships.
"""
from __future__ import annotations

import json
import os
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_CONFIG = REPO_ROOT / "desktop" / "config.default.json"
USER_CONFIG = Path.home() / "Library" / "Application Support" / "PadRemote" / "config.json"


@dataclass
class AccelCfg:
    curve: str = "quadratic"
    gain: float = 1.0


@dataclass
class TapCfg:
    tapMaxMs: int = 200
    tapMaxPx: float = 10.0
    doubleTapMs: int = 300
    pressMs: int = 250


@dataclass
class ScrollCfg:
    natural: bool = True
    momentum: bool = True
    speed: float = 1.0
    accel: float = 1.0
    horizontal: bool = True


@dataclass
class ZoomCfg:
    enabled: bool = True
    backend: str = "appZoom"
    threshold: float = 0.05


@dataclass
class Config:
    version: int = 1
    sensitivity: float = 1.0
    accel: AccelCfg = field(default_factory=AccelCfg)
    tap: TapCfg = field(default_factory=TapCfg)
    scroll: ScrollCfg = field(default_factory=ScrollCfg)
    zoom: ZoomCfg = field(default_factory=ZoomCfg)
    bindings: dict = field(default_factory=lambda: {
        "oneTap": "leftClick",
        "twoFingerTap": "rightClick",
        "threeFingerTap": "middleClick",
        "threeFingerSwipe": "none",
        "fourFingerSwipe": "none",
    })

    @staticmethod
    def from_dict(d: dict) -> "Config":
        c = Config()
        c.version = d.get("version", 1)
        c.sensitivity = float(d.get("sensitivity", 1.0))
        for name, cls in (("accel", AccelCfg), ("tap", TapCfg), ("scroll", ScrollCfg), ("zoom", ZoomCfg)):
            sub = d.get(name) or {}
            cur = getattr(c, name)
            for k, v in sub.items():
                if hasattr(cur, k):
                    setattr(cur, k, v)
        c.bindings.update(d.get("bindings") or {})
        return c


def seed_user_config() -> Path:
    """Copy the shipped defaults into the user config location if absent."""
    USER_CONFIG.parent.mkdir(parents=True, exist_ok=True)
    if not USER_CONFIG.exists() and DEFAULT_CONFIG.exists():
        USER_CONFIG.write_text(DEFAULT_CONFIG.read_text())
    return USER_CONFIG


def load(path: Path | None = None) -> Config:
    p = path or (USER_CONFIG if USER_CONFIG.exists() else DEFAULT_CONFIG)
    try:
        return Config.from_dict(json.loads(p.read_text()))
    except FileNotFoundError:
        return Config()
    except (json.JSONDecodeError, ValueError) as e:
        print(f"[padremote] config {p} is invalid ({e}); using defaults")
        return Config()


def mtime(path: Path) -> float:
    try:
        return os.path.getmtime(path)
    except OSError:
        return 0.0
