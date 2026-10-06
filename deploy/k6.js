import http from 'k6/http';
import { check, sleep } from 'k6';

// VUS is overridable so the documented 300-concurrency target can actually be
// produced instead of merely asserted:
//   VUS=300 deploy/loadtest.sh https://sibbs.cn
// The default matches the numbers in docs/performance.md.
const VUS = Number(__ENV.VUS || 64);
const DURATION = __ENV.DURATION || '30s';
const PATHNAME = __ENV.PATHNAME || '/api/projects';

export const options = {
  scenarios: {
    smoke: {
      executor: 'constant-vus',
      vus: VUS,
      duration: DURATION,
    },
  },
  // These are the whole point of using k6 here: unlike `ab`/`oha`, a breached
  // threshold makes the run fail.
  thresholds: {
    http_req_duration: ['p(95)<250'],
    http_req_failed: ['rate<0.01'],
  },
};

const BASE = __ENV.BASE || 'http://localhost:3000';

export default function () {
  const res = http.get(`${BASE}${PATHNAME}`);
  check(res, { '200 ok': (r) => r.status === 200 });
  sleep(0.01);
}
