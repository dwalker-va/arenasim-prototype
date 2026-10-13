"""NN rows of the branch against main's TeamPlan at the same seeds."""
import csv, gzip, sys


def read(p):
    op = gzip.open if p.endswith('.gz') else open
    with op(p, 'rt') as f:
        return {(r['label'], r['seed']): r for r in csv.DictReader(f) if r['label'].endswith('#NN')}


b, m = read(sys.argv[1]), read(sys.argv[2])
assert b.keys() == m.keys(), (len(b), len(m))
key = lambda r: (r['winner'], r['end_reason'], r['duration_secs'])
same = sum(key(b[k]) == key(m[k]) for k in b)
print(f"CONTROL: {same}/{len(b)} NN rows identical in winner, end reason and duration; "
      f"{sum(r['end_reason'] == 'kill' for r in b.values())} kills, "
      f"{len({r['duration_secs'] for r in b.values()})} distinct durations")
sys.exit(0 if same == len(b) else 1)
