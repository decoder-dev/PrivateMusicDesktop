# UI tests

Run the real UI in Chromium against a fake backend (`mock-backend.js`), served
with the same Content-Security-Policy as the app.

    npm install && npx playwright install chromium   # once
    npm test
    PM_SCREENSHOTS=/tmp/shots npm test                # also saves screenshots
