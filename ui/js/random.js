/** Uniform VK playback event ID in the inclusive range 1..2147483646. */
export function randomPlaybackId(fill = values => crypto.getRandomValues(values)) {
 const values = new Uint32Array(1);
 let value;
 do {
  fill(values);
  // Each 31-bit value has exactly two 32-bit preimages. Reject the two endpoints.
  value = values[0] >>> 1;
 } while (value === 0 || value === 0x7fffffff);
 return value;
}
