import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

fun main() {
    val client = OkHttpClient()

    val body = "quoi ? ça va\r\ntrès bien".toRequestBody("text/plain".toMediaType())

    val request = Request.Builder()
        .url("http://127.0.0.1:8765/items/7")
        .method("PATCH", body)
        .build()

    client.newCall(request).execute().use { response ->
        println(response.code)
        println(response.body?.string())
    }
}
