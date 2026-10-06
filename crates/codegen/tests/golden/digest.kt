// Auth OAuth 2.0 : ajoute le jeton obtenu dans l'en-tête Authorization.
// Digest : ajoute un Authenticator OkHttp (le défi vient du serveur).
import okhttp3.OkHttpClient
import okhttp3.Request

fun main() {
    val client = OkHttpClient()

    val request = Request.Builder()
        .url("http://127.0.0.1:8765/secure")
        .get()
        .build()

    client.newCall(request).execute().use { response ->
        println(response.code)
        println(response.body?.string())
    }
}
