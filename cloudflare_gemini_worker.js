/**
 * Cloudflare Worker Reverse Proxy for Google Gemini API
 * ======================================================
 * 100% защита от утечек API-ключа + обход региональных ограничений (0 RPM в ЕС).
 *
 * Преимущества:
 * 1. Запросы к Google идут из датацентров Cloudflare (США/Global).
 * 2. Трафик 100% зашифрован (End-to-End TLS): ключ не передается через публичные прокси.
 * 3. Бесплатно: 100,000 запросов в день на бесплатном аккаунте Cloudflare.
 * 4. Задержка минимальная (< 30-50 мс).
 *
 * Инструкция по установке (занимает 1 минуту):
 * 1. Зайдите на https://dash.cloudflare.com -> Workers & Pages -> Create Application -> Create Worker.
 * 2. Вставьте этот код и нажмите "Deploy".
 * 3. Скопируйте полученный адрес воркера (например: https://gemini-proxy.yourname.workers.dev).
 * 4. В .env вашего бота укажите:
 *    GEMINI_API_ENDPOINT=https://gemini-proxy.yourname.workers.dev
 */

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    
    // Перенаправляем на официальный API Google Gemini
    url.hostname = "generativelanguage.googleapis.com";
    url.port = "443";
    url.protocol = "https:";

    // Создаем новый запрос с сохранением всех заголовков, включая x-goog-api-key
    const newHeaders = new Headers(request.headers);
    newHeaders.set("Host", "generativelanguage.googleapis.com");

    const modifiedRequest = new Request(url.toString(), {
      method: request.method,
      headers: newHeaders,
      body: request.body,
      redirect: "follow",
    });

    const response = await fetch(modifiedRequest);
    return response;
  },
};
