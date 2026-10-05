import http from 'k6/http';
import { check, sleep } from 'k6';

export const options = {
  scenarios: {
    smoke: {
      executor: 'constant-vus',
      vus: 64,
      duration: '30s',
    },
  },
  thresholds: {
    http_req_duration: ['p(95)<250'],
    http_req_failed: ['rate<0.01'],
  },
};

const BASE = __ENV.BASE || 'http://localhost:3000';

export default function () {
  const res = http.get(`${BASE}/api/projects`);
  check(res, { '200 ok': (r) => r.status === 200 });
  sleep(0.01);
}
