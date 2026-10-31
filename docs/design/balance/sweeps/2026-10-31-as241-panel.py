#!/usr/bin/env python3
"""AS-241: the Nagrand Druid panel under both AI profiles.

usage: 2026-10-31-as241-panel.py > panel.jsonl

Every config of the AS-164 Nagrand Druid panel (`Legacy`), followed by the same
config under `TeamPlan` with `@TeamPlan` appended to its label. Run it with
`tests/milestone_mechanisms.rs` (MECH_IN=panel.jsonl), once per arm.
"""
import json
import os
import sys

here = os.path.dirname(os.path.abspath(__file__))
for line in open(os.path.join(here, '2026-10-10-as164-mech_nagrand.jsonl')):
    config = json.loads(line)
    sys.stdout.write(json.dumps(config) + '\n')
    teamplan = dict(config, ai_profile='TeamPlan', label=config['label'] + '@TeamPlan')
    sys.stdout.write(json.dumps(teamplan) + '\n')
