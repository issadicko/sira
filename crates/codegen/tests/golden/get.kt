import okhttp3.Headers
import okhttp3.OkHttpClient
import okhttp3.Request

fun main() {
    val client = OkHttpClient()

    val request = Request.Builder()
        .url("http://127.0.0.1:8765/items?x=1&y=a%20b")
        .get()
        .headers(Headers.Builder()
            .add("Accept", "application/json")
            .addUnsafeNonAscii("X-Odd", "valé \"double\" 'simple' \$dollar \\anti `tick`")
            .build())
        .build()

    client.newCall(request).execute().use { response ->
        println(response.code)
        println(response.body?.string())
    }
}
