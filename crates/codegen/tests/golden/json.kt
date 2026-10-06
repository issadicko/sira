import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

fun main() {
    val client = OkHttpClient()

    val body = "{\n  \"name\": \"Aminata \\\"Ami\\\" Diallo\",\n  \"note\": \"café \$HOME \${x} 日本語\\n\",\n  \"path\": \"C:\\\\temp\"\n}".toRequestBody("application/json; charset=utf-8".toMediaType())

    val request = Request.Builder()
        .url("http://127.0.0.1:8765/users")
        .method("POST", body)
        .addHeader("Authorization", "Bearer abc.def")
        .build()

    client.newCall(request).execute().use { response ->
        println(response.code)
        println(response.body?.string())
    }
}
