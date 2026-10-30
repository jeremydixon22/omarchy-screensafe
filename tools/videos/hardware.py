"""Compatibility import for the shared trusted GPU authority."""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from screensafe_gpu import RenderNode, VERIFY_DEVICE, select_node
