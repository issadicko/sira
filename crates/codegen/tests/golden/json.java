import okhttp3.MediaType;
import okhttp3.OkHttpClient;
import okhttp3.Request;
import okhttp3.RequestBody;
import okhttp3.Response;

public class Main {
    public static void main(String[] args) throws Exception {
        OkHttpClient client = new OkHttpClient();

        MediaType mediaType = MediaType.parse("application/json; charset=utf-8");
        RequestBody body = RequestBody.create("{\n  \"name\": \"Aminata \\\"Ami\\\" Diallo\",\n  \"note\": \"café $HOME ${x} 日本語\\n\",\n  \"path\": \"C:\\\\temp\"\n}", mediaType);

        Request request = new Request.Builder()
            .url("http://127.0.0.1:8765/users")
            .method("POST", body)
            .addHeader("Authorization", "Bearer abc.def")
            .build();

        try (Response response = client.newCall(request).execute()) {
            System.out.println(response.code());
            System.out.println(response.body().string());
        }
    }
}
