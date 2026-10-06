import okhttp3.OkHttpClient
import okhttp3.Request

fun main() {
    val client = OkHttpClient()

    val request = Request.Builder()
        .url("http://127.0.0.1:8765/items/7")
        .method("DELETE", null)
        .addHeader("X-Reason", "cleanup")
        .build()

    client.newCall(request).execute().use { response ->
        println(response.code)
        println(response.body?.string())
    }
}
