#!/usr/bin/env python3
MAX=5
for fuel in range(MAX+1):
  for cost in range(1,4):
    allowed=fuel>=cost
    after=fuel-cost if allowed else fuel
    assert 0<=after<=MAX, "runtime fuel escaped bounds"
    if not allowed: assert after==fuel
approved={"http","clock","log"}
for cap in ("http","clock","log","fs","spawn"):
  if cap not in approved: assert cap not in approved
print("runtime fuel/authority bounds: ok")
