/** Не больше `limit` каталожных чтений VK одновременно. Команды плеера сюда не попадают. */
export function createCatalogueGate(limit = 2) {
  let active = 0;
  const waiting = [];
  return function gate(run) {
    return new Promise((resolve, reject) => {
      const start = () => {
        active += 1;
        Promise.resolve()
          .then(run)
          .then(resolve, reject)
          .finally(() => {
            active -= 1;
            waiting.shift()?.();
          });
      };
      if (active < limit) start();
      else waiting.push(start);
    });
  };
}
