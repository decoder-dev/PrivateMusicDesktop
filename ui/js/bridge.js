// Единственный файл, который разговаривает с Таури. Все остальное использует эти функции, что позволяет
// протестировать UI на поддельном бэкэнде.

const tauri = () => {
  const t = window.__TAURI__;
  if (!t) throw new Error("Tauri API is not available");
  return t;
};

export const invoke = (command, args) => tauri().core.invoke(command, args);

/** Подписывается на серверное событие; превращается в функцию отписки. */
export const listen = (event, handler) => tauri().event.listen(event, (e) => handler(e.payload));
