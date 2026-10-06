import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

fun main() {
    val client = OkHttpClient()

    val body = "grant=password&user=x%40y.test&scope=a+b".toRequestBody("application/x-www-form-urlencoded".toMediaType())

    val request = Request.Builder()
        .url("http://127.0.0.1:8765/login")
        .method("POST", body)
        .build()

    client.newCall(request).execute().use { response ->
        println(response.code)
        println(response.body?.string())
    }
}
