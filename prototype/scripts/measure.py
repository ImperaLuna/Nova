#!/usr/bin/env python3
"""Memory, idle CPU and GPU memory of running processes (IMP-369 prototype).

  measure.py --seconds 60 PID [PID...]

Pss comes from /proc/PID/smaps_rollup, CPU from utime+stime over the window,
GPU memory from NVML (nvidia-smi segfaults on RyzenShine, the library works).
"""

import argparse
import ctypes
import os
import time


def pss_kb(pid: int) -> int:
    with open(f"/proc/{pid}/smaps_rollup") as f:
        for line in f:
            if line.startswith("Pss:"):
                return int(line.split()[1])
    return 0


def cpu_ticks(pid: int) -> int:
    with open(f"/proc/{pid}/stat") as f:
        fields = f.read().rsplit(")", 1)[1].split()
    # utime and stime are fields 14 and 15; after the ")" split they are 11, 12.
    return int(fields[11]) + int(fields[12])


def gpu_mb() -> dict:
    """Per-process GPU memory in MB from NVML, or {} without it."""
    try:
        nvml = ctypes.CDLL("libnvidia-ml.so.1")
    except OSError:
        return {}

    class Info(ctypes.Structure):
        _fields_ = [
            ("pid", ctypes.c_uint),
            ("usedGpuMemory", ctypes.c_ulonglong),
            ("gpuInstanceId", ctypes.c_uint),
            ("computeInstanceId", ctypes.c_uint),
        ]

    if nvml.nvmlInit_v2() != 0:
        return {}
    out = {}
    try:
        handle = ctypes.c_void_p()
        nvml.nvmlDeviceGetHandleByIndex_v2(0, ctypes.byref(handle))
        for name in ("nvmlDeviceGetGraphicsRunningProcesses_v3", "nvmlDeviceGetComputeRunningProcesses_v3"):
            fn = getattr(nvml, name, None)
            if fn is None:
                continue
            count = ctypes.c_uint(64)
            infos = (Info * 64)()
            if fn(handle, ctypes.byref(count), infos) == 0:
                for i in range(count.value):
                    out[infos[i].pid] = max(out.get(infos[i].pid, 0), infos[i].usedGpuMemory // (1024 * 1024))
    finally:
        nvml.nvmlShutdown()
    return out


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("pids", nargs="+", type=int)
    parser.add_argument("--seconds", type=float, default=60)
    args = parser.parse_args()
    hz = os.sysconf("SC_CLK_TCK")
    start = {pid: cpu_ticks(pid) for pid in args.pids}
    time.sleep(args.seconds)
    gpu = gpu_mb()
    for pid in args.pids:
        with open(f"/proc/{pid}/comm") as f:
            comm = f.read().strip()
        cpu = 100.0 * (cpu_ticks(pid) - start[pid]) / hz / args.seconds
        vram = gpu.get(pid)
        print(
            f"{comm} pid={pid} pss={pss_kb(pid) / 1024:.1f}MB cpu={cpu:.2f}% "
            f"over {args.seconds:.0f}s gpu={'%dMB' % vram if vram is not None else 'n/a'}"
        )


if __name__ == "__main__":
    main()
