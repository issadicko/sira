// Auth OAuth 2.0 : ajoute le jeton obtenu dans l'en-tête Authorization.
// Digest : ajoute un Authenticator OkHttp (le défi vient du serveur).
import okhttp3.MediaType;
import okhttp3.OkHttpClient;
import okhttp3.Request;
import okhttp3.RequestBody;
import okhttp3.Response;

public class Main {
    public static void main(String[] args) throws Exception {
        OkHttpClient client = new OkHttpClient();

        Request request = new Request.Builder()
            .url("http://127.0.0.1:8765/secure")
            .get()
            .build();

        try (Response response = client.newCall(request).execute()) {
            System.out.println(response.code());
            System.out.println(response.body().string());
        }
    }
}
